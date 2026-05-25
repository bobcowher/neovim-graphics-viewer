local M = {}

local function plugin_root()
    local src = debug.getinfo(1, "S").source:sub(2)
    return vim.fn.fnamemodify(src, ":h:h:h")
end

function M.setup(_opts)
    local viewer = require("nvim-gfx.viewer")

    vim.api.nvim_create_user_command("ViewImage", function(a)
        viewer.open(a.args)
    end, { nargs = 1, complete = "file", desc = "View an image file" })

    vim.api.nvim_create_user_command("NvimGfxBuild", function()
        local root = plugin_root()
        local cmd = string.format(
            "cd %s && cargo build --release && mkdir -p bin && cp target/release/nvim-gfx bin/nvim-gfx",
            vim.fn.shellescape(root)
        )
        vim.notify("nvim-gfx: building (check :messages for result)...", vim.log.levels.INFO)
        vim.fn.jobstart({ "sh", "-c", cmd }, {
            on_exit = function(_, code)
                if code == 0 then
                    vim.notify("nvim-gfx: build complete — bin/nvim-gfx ready", vim.log.levels.INFO)
                else
                    vim.notify("nvim-gfx: build failed (code " .. code .. ")", vim.log.levels.ERROR)
                end
            end,
        })
    end, { desc = "Build nvim-gfx Rust binary" })

    vim.api.nvim_create_autocmd("BufReadPost", {
        pattern  = { "*.png", "*.jpg", "*.jpeg", "*.webp",
                     "*.mp4", "*.mkv", "*.webm", "*.avi", "*.mov", "*.m4v" },
        callback = function(ev) viewer.open(ev.file) end,
        desc     = "Auto-preview image and video files with nvim-gfx",
    })
end

return M
