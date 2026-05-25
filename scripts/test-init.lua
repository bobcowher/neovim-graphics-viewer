-- Injects nvim-gfx into the runtime path, then sources your real config.
-- nvim -u replaces init.lua selection; dofile sources it explicitly.
local plugin_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h")
dofile(vim.fn.expand("~/.config/nvim/init.lua"))
-- Prepend after real config loads so lazy.nvim / rtp resets don't drop our path.
vim.opt.rtp:prepend(plugin_dir)
require("nvim-gfx").setup()
