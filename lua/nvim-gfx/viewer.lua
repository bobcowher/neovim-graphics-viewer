local geometry = require("nvim-gfx.geometry")

local M = {}

local VIDEO_EXTS = { mp4=true, mkv=true, webm=true, avi=true, mov=true, m4v=true }

local state = {
    job_id     = nil,
    bufnr      = nil,
    orig_bufnr = nil,
    winid      = nil,
    aug_id     = nil,
    path       = nil,
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
        vim.fn.chansend(state.job_id, vim.json.encode(cmd) .. "\n")
    end
end

local function cleanup()
    if state.aug_id then
        pcall(vim.api.nvim_del_augroup_by_id, state.aug_id)
        state.aug_id = nil
    end
    if state.orig_bufnr and vim.api.nvim_buf_is_valid(state.orig_bufnr) then
        pcall(vim.api.nvim_buf_delete, state.orig_bufnr, { force = true })
    end
    if state.bufnr and vim.api.nvim_buf_is_valid(state.bufnr) then
        vim.api.nvim_buf_delete(state.bufnr, { force = true })
    end
    state.job_id     = nil
    state.bufnr      = nil
    state.orig_bufnr = nil
    state.winid      = nil
    state.path       = nil
end

local function on_stdout(_, data, _)
    for _, line in ipairs(data) do
        if line ~= "" then
            local ok, ev = pcall(vim.json.decode, line)
            if ok and type(ev) == "table" and ev.event == "error" then
                vim.notify("nvim-gfx: " .. (ev.msg or "unknown error"), vim.log.levels.ERROR)
                M.close()
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
    vim.keymap.set("n", "q",        function() M.close() end, o)
    vim.keymap.set("n", "+",        function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "=",        function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "-",        function() send({ cmd = "zoom", factor = 0.8  }) end, o)
    vim.keymap.set("n", "r",        function() send({ cmd = "reset" }) end, o)
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
    vim.keymap.set("n", "+",        function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "=",        function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "-",        function() send({ cmd = "zoom", factor = 0.8  }) end, o)
    vim.keymap.set("n", "r",        function() send({ cmd = "rewind" }) end, o)
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

    -- Wipe the original image buffer immediately so no other plugin can find
    -- and load the raw binary content into any window.
    if state.orig_bufnr and vim.api.nvim_buf_is_valid(state.orig_bufnr) then
        pcall(vim.api.nvim_buf_delete, state.orig_bufnr, { force = true })
        state.orig_bufnr = nil
    end

    if is_video(path) then
        set_video_keymaps(bufnr)
    else
        set_image_keymaps(bufnr)
    end

    -- When spawned via jobstart, the child has no controlling terminal,
    -- so /dev/tty fails. Resolve Neovim's own stdout fd to get the real pts path.
    local nvim_tty = vim.fn.resolve("/proc/" .. vim.fn.getpid() .. "/fd/1")

    state.job_id = vim.fn.jobstart({ bin }, {
        on_stdout       = on_stdout,
        on_stderr       = on_stderr,
        on_exit         = on_exit,
        stdout_buffered = false,
        env             = { NVIM_GFX_TTY = nvim_tty },
    })

    local geo = geometry.win_geometry(state.winid)
    send({ cmd = "show", path = path,
           row = geo.row, col = geo.col,
           width = geo.width, height = geo.height })

    local function on_resize()
        if not state.job_id then return end
        local g = geometry.win_geometry(state.winid)
        send({ cmd = "show", path = state.path,
               row = g.row, col = g.col,
               width = g.width, height = g.height })
    end

    state.aug_id = vim.api.nvim_create_augroup("NvimGfxResize" .. bufnr, { clear = true })
    vim.api.nvim_create_autocmd("VimResized", { group = state.aug_id, callback = on_resize })
    vim.api.nvim_create_autocmd("WinResized", { group = state.aug_id, callback = on_resize })
end

function M.close()
    if state.job_id then
        send({ cmd = "quit" })
        vim.fn.jobwait({ state.job_id }, 200)
        vim.fn.jobstop(state.job_id)
    end
    cleanup()
end

return M
