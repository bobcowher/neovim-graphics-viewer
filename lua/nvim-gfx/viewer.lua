local geometry = require("nvim-gfx.geometry")

local M = {}

local VIDEO_EXTS = { mp4=true, mkv=true, webm=true, avi=true, mov=true, m4v=true }

local state = {
    job_id       = nil,
    bufnr        = nil,
    orig_bufnr   = nil,
    winid        = nil,
    aug_id       = nil,
    path         = nil,
    redraw_timer = nil,
    last_geo     = nil,
}

local function is_video(path)
    local ext = path:match("%.(%w+)$")
    return ext ~= nil and VIDEO_EXTS[ext:lower()] == true
end

local function binary_path()
    local src = debug.getinfo(1, "S").source:sub(2)
    local root = vim.fn.fnamemodify(src, ":h:h:h")
    return root .. "/bin/nvim-gfx"
end

local function send(cmd)
    if state.job_id then
        local sent = vim.fn.chansend(state.job_id, vim.json.encode(cmd) .. "\n")
        if sent == 0 then
            vim.notify("nvim-gfx: chansend failed!", vim.log.levels.WARN)
        end
    end
end

local function schedule_redraw()
    if state.redraw_timer then
        vim.loop.timer_stop(state.redraw_timer)
        state.redraw_timer:close()
    end
    state.redraw_timer = vim.loop.new_timer()
    state.redraw_timer:start(100, 100, function()
        vim.schedule(function()
            if state.job_id and state.winid and vim.api.nvim_win_is_valid(state.winid) then
                local g = geometry.win_geometry(state.winid)
                local prev = state.last_geo
                if not prev or g.row ~= prev.row or g.col ~= prev.col
                    or g.width ~= prev.width or g.height ~= prev.height
                then
                    state.last_geo = { row = g.row, col = g.col, width = g.width, height = g.height }
                    send({ cmd = "show", path = state.path,
                           row = g.row, col = g.col,
                           width = g.width, height = g.height })
                end
            end
        end)
    end)
end

local function cleanup()
    if state.redraw_timer then
        vim.loop.timer_stop(state.redraw_timer)
        state.redraw_timer:close()
        state.redraw_timer = nil
    end
    if state.aug_id then
        pcall(vim.api.nvim_del_augroup_by_id, state.aug_id)
        state.aug_id = nil
    end
    if state.orig_bufnr and vim.api.nvim_buf_is_valid(state.orig_bufnr) then
        local b = state.orig_bufnr
        state.orig_bufnr = nil
        pcall(vim.api.nvim_buf_delete, b, { force = true })
    end
    if state.bufnr and vim.api.nvim_buf_is_valid(state.bufnr) then
        local b = state.bufnr
        state.bufnr = nil  -- nil before delete to prevent BufWipeout re-entrancy
        pcall(vim.api.nvim_buf_delete, b, { force = true })
    end
    state.job_id       = nil
    state.bufnr        = nil
    state.orig_bufnr   = nil
    state.winid        = nil
    state.path         = nil
    state.last_geo     = nil
end

local function fmt_time(secs)
    local m = math.floor(secs / 60)
    local s = math.floor(secs % 60)
    return string.format("%02d:%02d", m, s)
end

local function on_stdout(_, data, _)
    for _, line in ipairs(data) do
        if line ~= "" then
            local ok, ev = pcall(vim.json.decode, line)
            if ok and type(ev) == "table" then
                if ev.event == "error" then
                    vim.notify("nvim-gfx: " .. (ev.msg or "unknown error"), vim.log.levels.ERROR)
                    M.close()
                elseif ev.event == "time" and state.bufnr and vim.api.nvim_buf_is_valid(state.bufnr) then
                    local status = ev.playing and "playing" or "paused"
                    local pos = fmt_time(ev.position)
                    local dur = (ev.duration or 0) > 0 and fmt_time(ev.duration) or "--:--"
                    pcall(vim.api.nvim_buf_set_name, state.bufnr, status .. " " .. pos .. " / " .. dur)
                end
            end
        end
    end
end

local function on_stderr(_, data, _)
    for _, line in ipairs(data) do
        if line ~= "" then
            vim.notify("nvim-gfx: " .. line, vim.log.levels.WARN)
        end
    end
end

local function on_exit(_, code, _)
    if code ~= 0 then
        vim.notify("nvim-gfx: binary exited with code " .. code, vim.log.levels.WARN)
    end
    cleanup()
end

local function set_image_keymaps(bufnr)
    local o = { noremap = true, silent = true, buffer = bufnr }
    vim.keymap.set("n", "q",   function() M.close() end, o)
    vim.keymap.set("n", "+",   function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "-",   function() send({ cmd = "zoom", factor = 0.8  }) end, o)
    vim.keymap.set("n", "=",   function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "r",   function() send({ cmd = "reset" }) end, o)
    vim.keymap.set("n", "h",        function() send({ cmd = "pan", dx = -1, dy =  0 }) end, o)
    vim.keymap.set("n", "<Left>",   function() send({ cmd = "pan", dx = -1, dy =  0 }) end, o)
    vim.keymap.set("n", "l",        function() send({ cmd = "pan", dx =  1, dy =  0 }) end, o)
    vim.keymap.set("n", "<Right>",  function() send({ cmd = "pan", dx =  1, dy =  0 }) end, o)
    vim.keymap.set("n", "k",        function() send({ cmd = "pan", dx =  0, dy = -1 }) end, o)
    vim.keymap.set("n", "<Up>",     function() send({ cmd = "pan", dx =  0, dy = -1 }) end, o)
    vim.keymap.set("n", "j",        function() send({ cmd = "pan", dx =  0, dy =  1 }) end, o)
    vim.keymap.set("n", "<Down>",   function() send({ cmd = "pan", dx =  0, dy =  1 }) end, o)
end

local function set_video_keymaps(bufnr)
    local o = { noremap = true, silent = true, buffer = bufnr }
    vim.keymap.set("n", "q",        function() M.close() end, o)
    vim.keymap.set("n", "p",        function() send({ cmd = "play_pause" }) end, o)
    vim.keymap.set("n", "<Space>",  function() send({ cmd = "play_pause" }) end, o)
    vim.keymap.set("n", "h",        function() send({ cmd = "seek", delta = -1  }) end, o)
    vim.keymap.set("n", "<Left>",   function() send({ cmd = "seek", delta = -1  }) end, o)
    vim.keymap.set("n", "l",        function() send({ cmd = "seek", delta =  1  }) end, o)
    vim.keymap.set("n", "<Right>",  function() send({ cmd = "seek", delta =  1  }) end, o)
    vim.keymap.set("n", "j",        function() send({ cmd = "seek", delta = -10 }) end, o)
    vim.keymap.set("n", "<Down>",   function() send({ cmd = "seek", delta = -10 }) end, o)
    vim.keymap.set("n", "k",        function() send({ cmd = "seek", delta =  10 }) end, o)
    vim.keymap.set("n", "<Up>",     function() send({ cmd = "seek", delta =  10 }) end, o)
    vim.keymap.set("n", "+",   function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "-",   function() send({ cmd = "zoom", factor = 0.8  }) end, o)
    vim.keymap.set("n", "=",   function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "r",   function() send({ cmd = "rewind" }) end, o)
end

function M.open(path)
    path = vim.fn.expand(path)
    if vim.fn.filereadable(path) == 0 then
        vim.notify("nvim-gfx: file not readable: " .. path, vim.log.levels.ERROR)
        return
    end

    local bin = binary_path()
    if vim.fn.executable(bin) == 0 then
        vim.notify("nvim-gfx: binary not found, run :NvimGfxBuild", vim.log.levels.ERROR)
        return
    end

    if state.job_id then M.close() end

    state.winid      = vim.api.nvim_get_current_win()
    state.orig_bufnr = vim.api.nvim_get_current_buf()
    state.path       = path

    local bufnr = vim.api.nvim_create_buf(false, true)
    state.bufnr = bufnr
    vim.api.nvim_win_set_buf(state.winid, bufnr)
    vim.bo[bufnr].bufhidden  = "wipe"
    vim.bo[bufnr].filetype   = "nvim-gfx"
    vim.bo[bufnr].modifiable = false
    vim.wo[state.winid].number = false
    vim.wo[state.winid].relativenumber = false
    vim.wo[state.winid].signcolumn = "no"

    -- Wipe the original image buffer immediately so no other plugin can find
    -- and load the raw binary content into any window.
    local orig_name = vim.api.nvim_buf_get_name(state.orig_bufnr)
    if orig_name ~= "" and orig_name:match("%.(png|jpg|jpeg|webp|mp4|mkv|webm|avi|mov|m4v)$") then
        pcall(vim.api.nvim_buf_delete, state.orig_bufnr, { force = true })
        state.orig_bufnr = nil
    end

    if is_video(path) then
        set_video_keymaps(bufnr)
    else
        set_image_keymaps(bufnr)
    end

    local is_macos = vim.loop.os_uname().sysname == "Darwin"
    local tty = vim.fn.resolve(is_macos and "/dev/fd/1" or "/proc/self/fd/1")
    if tty == "" or tty == "/dev/fd/1" or tty == "/proc/self/fd/1" then
        vim.notify("nvim-gfx: could not resolve TTY path", vim.log.levels.ERROR)
        return
    end

    state.job_id = vim.fn.jobstart({ bin }, {
        env             = { NVIM_GFX_TTY = tty },
        on_stdout       = on_stdout,
        on_stderr       = on_stderr,
        on_exit         = on_exit,
        stdout_buffered = false,
    })

    local geo = geometry.win_geometry(state.winid)
    send({ cmd = "show", path = path,
           row = geo.row, col = geo.col,
           width = geo.width, height = geo.height })

    local function on_resize()
        if not state.job_id then return end
        if not vim.api.nvim_win_is_valid(state.winid) then
            vim.schedule(function() M.close() end)
            return
        end
        local g = geometry.win_geometry(state.winid)
        send({ cmd = "show", path = state.path,
               row = g.row, col = g.col,
               width = g.width, height = g.height })
    end

    state.aug_id = vim.api.nvim_create_augroup("NvimGfxResize" .. bufnr, { clear = true })
    vim.api.nvim_create_autocmd("VimResized",  { group = state.aug_id, callback = on_resize })
    vim.api.nvim_create_autocmd("WinResized",  { group = state.aug_id, callback = on_resize })
    vim.api.nvim_create_autocmd("CursorMoved", { group = state.aug_id, callback = schedule_redraw })
    vim.api.nvim_create_autocmd("WinScrolled", { group = state.aug_id, callback = schedule_redraw })
    vim.api.nvim_create_autocmd("WinEnter",    { group = state.aug_id, callback = schedule_redraw })
    vim.api.nvim_create_autocmd("BufEnter",    { group = state.aug_id, callback = schedule_redraw })
    vim.api.nvim_create_autocmd("ModeChanged", { group = state.aug_id, callback = schedule_redraw })
    vim.api.nvim_create_autocmd("BufWipeout",  { group = state.aug_id, buf = bufnr,
                                                  callback = function() M.close() end })
end

function M.close()
    local jid = state.job_id
    if jid then
        state.job_id = nil
        vim.fn.chansend(jid, vim.json.encode({ cmd = "quit" }) .. "\n")
        vim.fn.jobwait({ jid }, 200)
        pcall(vim.fn.jobstop, jid)
    end
    cleanup()
end

return M
