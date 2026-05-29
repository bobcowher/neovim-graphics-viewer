local geometry = require("nvim-gfx.geometry")

local M = {}

local VIDEO_EXTS = { mp4=true, mkv=true, webm=true, avi=true, mov=true, m4v=true }
local IMAGE_EXTS = { png=true, jpg=true, jpeg=true, webp=true, gif=true, bmp=true }

local state = {
    job_id       = nil,
    bufnr        = nil,
    orig_bufnr   = nil,
    winid        = nil,
    aug_id       = nil,
    path         = nil,
    redraw_timer = nil,
    last_geo     = nil,
    is_video     = false,
    playing      = false,
    auto_paused  = false,
}

local function is_video(path)
    local ext = path:match("%.(%w+)$")
    return ext ~= nil and VIDEO_EXTS[ext:lower()] == true
end

local function is_media(path)
    local ext = path:match("%.(%w+)$")
    if not ext then return false end
    ext = ext:lower()
    return IMAGE_EXTS[ext] == true or VIDEO_EXTS[ext] == true
end

function M.is_active()
    return state.job_id ~= nil
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
    state.job_id       = nil
    state.bufnr        = nil
    state.orig_bufnr   = nil
    state.winid        = nil
    state.path         = nil
    state.last_geo     = nil
    state.is_video     = false
    state.playing      = false
    state.auto_paused  = false
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
                    M.close("error_event")
                elseif ev.event == "time" and state.bufnr and vim.api.nvim_buf_is_valid(state.bufnr) then
                    state.playing = ev.playing == true
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
    vim.keymap.set("n", "q",   function() M.close("keymap_q_image") end, o)
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
    vim.keymap.set("n", "q",        function() M.close("keymap_q_video") end, o)
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

    if state.job_id then
        if state.path == path then
            -- NvimTree's open_in_new_window fires BufReadPost twice for a
            -- single file open. Ignore the duplicate so we don't spawn a
            -- second renderer that races the first on the shared image ID.
            return
        end
        M.close("open_reopen")
    end

    state.winid      = vim.api.nvim_get_current_win()
    state.orig_bufnr = vim.api.nvim_get_current_buf()
    state.path       = path

    local bufnr = vim.api.nvim_create_buf(false, false)
    state.bufnr = bufnr
    vim.api.nvim_win_set_buf(state.winid, bufnr)
    vim.bo[bufnr].bufhidden  = "wipe"
    vim.bo[bufnr].filetype   = "nvim-gfx"
    vim.bo[bufnr].modifiable = false
    vim.bo[bufnr].swapfile   = false
    vim.wo[state.winid].number = false
    vim.wo[state.winid].relativenumber = false
    vim.wo[state.winid].signcolumn = "no"

    -- Wipe any buffer Neovim/NvimTree loaded with the raw image bytes, matched
    -- by filename. (BufReadPost runs after Neovim has read the file into some
    -- buffer; that buffer isn't necessarily the one we captured as orig_bufnr.)
    -- Removing it stops the raw bytes resurfacing as text and forces a fresh
    -- BufReadPost on reopen instead of NvimTree showing the stale buffer.
    local target = vim.fn.fnamemodify(path, ":p")
    for _, b in ipairs(vim.api.nvim_list_bufs()) do
        if b ~= bufnr then
            local bname = vim.api.nvim_buf_get_name(b)
            if bname ~= "" and vim.fn.fnamemodify(bname, ":p") == target then
                if b == state.orig_bufnr then state.orig_bufnr = nil end
                pcall(vim.api.nvim_buf_delete, b, { force = true })
            end
        end
    end

    state.is_video = is_video(path)
    state.playing  = state.is_video   -- the binary auto-plays on open
    if state.is_video then
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
            vim.schedule(function() M.close("on_resize_invalid_win") end)
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
    -- The viewer buffer left its window (window closed, or a buffer replaced it
    -- in-place). bufhidden=wipe means this fires as the buffer is wiped. Tear
    -- down the renderer; do NOT block here (jobstop is scheduled).
    vim.api.nvim_create_autocmd("BufWipeout", {
        group    = state.aug_id,
        buffer   = bufnr,
        callback = function()
            if state.redraw_timer then
                vim.loop.timer_stop(state.redraw_timer)
                state.redraw_timer:close()
                state.redraw_timer = nil
            end
            local jid = state.job_id
            if jid then
                state.job_id = nil
                vim.fn.chansend(jid, vim.json.encode({ cmd = "quit" }) .. "\n")
                vim.schedule(function() pcall(vim.fn.jobstop, jid) end)
            end
        end,
    })
    vim.api.nvim_create_autocmd("BufEnter", {
        group    = state.aug_id,
        buffer   = bufnr,
        callback = function()
            if state.job_id then schedule_redraw() end
        end,
    })

    -- Video only: pause playback when the viewer window loses focus. A playing
    -- video redraws every frame, and each frame repositions the terminal
    -- cursor; with focus elsewhere (e.g. the sidebar) that fights Neovim's
    -- cursor and shows as rapid flicker. Auto-resume when focus returns.
    if state.is_video then
        vim.api.nvim_create_autocmd({ "WinLeave", "BufLeave" }, {
            group    = state.aug_id,
            buffer   = bufnr,
            callback = function()
                if state.job_id and state.playing and not state.auto_paused then
                    state.auto_paused = true
                    send({ cmd = "play_pause" })
                end
            end,
        })
        vim.api.nvim_create_autocmd({ "WinEnter", "BufEnter" }, {
            group    = state.aug_id,
            buffer   = bufnr,
            callback = function()
                if state.job_id and state.auto_paused then
                    state.auto_paused = false
                    send({ cmd = "play_pause" })
                end
            end,
        })
    end
end

function M.close(reason)
    local jid = state.job_id
    if jid then
        state.job_id = nil
        vim.fn.chansend(jid, vim.json.encode({ cmd = "quit" }) .. "\n")
        vim.fn.jobwait({ jid }, 200)
        pcall(vim.fn.jobstop, jid)
    end

    local winid = state.winid
    local bufnr = state.bufnr
    local orig_bufnr = state.orig_bufnr

    cleanup()

    if winid and vim.api.nvim_win_is_valid(winid) then
        if orig_bufnr and vim.api.nvim_buf_is_valid(orig_bufnr) then
            vim.api.nvim_win_set_buf(winid, orig_bufnr)
        else
            pcall(vim.api.nvim_win_set_buf, winid, vim.api.nvim_create_buf(true, true))
        end
    end

    if bufnr and vim.api.nvim_buf_is_valid(bufnr) then
        pcall(vim.api.nvim_buf_delete, bufnr, { force = true })
    end

    -- On explicit quit (q), if the NvimTree sidebar is open, bounce focus to it
    -- instead of leaving the cursor in the now-empty viewer window. Other close
    -- paths (opening another file, window closed) already land focus correctly.
    if reason == "keymap_q_image" or reason == "keymap_q_video" then
        for _, w in ipairs(vim.api.nvim_list_wins()) do
            local wb = vim.api.nvim_win_get_buf(w)
            if vim.api.nvim_buf_is_valid(wb) and vim.bo[wb].filetype == "NvimTree" then
                pcall(vim.api.nvim_set_current_win, w)
                break
            end
        end
    end
end

-- Dedicated-viewer behavior: opening any other real file closes the image.
-- Called from a global BufWinEnter autocmd. Focusing the sidebar or another
-- window does NOT trigger this (BufWinEnter fires on display, not focus), and
-- special buffers (NvimTree, prompts, [No Name]) and media files are ignored.
function M.on_other_buf(bufnr)
    if not state.job_id then return end
    if bufnr == state.bufnr then return end
    if not (bufnr and vim.api.nvim_buf_is_valid(bufnr)) then return end
    if vim.bo[bufnr].buftype ~= "" then return end
    local name = vim.api.nvim_buf_get_name(bufnr)
    if name == "" then return end
    if is_media(name) then return end       -- media opens are handled by M.open
    vim.schedule(function() M.close("other_file_opened") end)
end

return M
