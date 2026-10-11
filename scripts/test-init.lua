-- Loads this checkout of nvim-gfx on top of your real config.
-- nvim -u replaces init.lua selection; dofile sources it explicitly.
local plugin_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h")
dofile(vim.fn.expand("~/.config/nvim/init.lua"))

-- The real config may already have loaded an installed nvim-gfx (e.g. a pinned
-- lazy.nvim copy). Evict its cached modules and autocmds so only this checkout
-- reacts to file opens.
for name in pairs(package.loaded) do
    if name == "nvim-gfx" or name:match("^nvim%-gfx%.") then
        package.loaded[name] = nil
    end
end
for _, au in ipairs(vim.api.nvim_get_autocmds({})) do
    if au.desc and au.desc:find("nvim-gfx", 1, true) then
        vim.api.nvim_del_autocmd(au.id)
    end
end

-- Pin every nvim-gfx module to this checkout via package.preload, which Lua
-- consults before any path search. A runtimepath prepend is not enough:
-- vim.loader's cached module index keeps resolving to the installed copy.
local lua_dir = plugin_dir .. "/lua/"
for _, file in ipairs(vim.fn.globpath(lua_dir, "nvim-gfx/**/*.lua", false, true)) do
    local mod = file:sub(#lua_dir + 1):gsub("%.lua$", ""):gsub("/init$", ""):gsub("/", ".")
    package.preload[mod] = function() return dofile(file) end
end

vim.opt.rtp:prepend(plugin_dir)
require("nvim-gfx").setup()
