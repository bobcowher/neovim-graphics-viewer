local M = {}

--- Return {x, y, w, h} in terminal cells for the given window handle.
--- x = leftmost column, y = topmost row (0-indexed from terminal top-left).
--- w = width in columns, h = height in rows.
function M.win_geometry(win)
    win = win or 0
    local pos = vim.api.nvim_win_get_position(win)
    local w = vim.api.nvim_win_get_width(win)
    local h = vim.api.nvim_win_get_height(win)
    return {
        x    = pos[2],
        y    = pos[1],
        w    = w,
        h    = h,
        cols = vim.o.columns,
        rows = vim.o.lines,
    }
end

return M
