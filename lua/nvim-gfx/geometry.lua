local M = {}

--- Return cell-space geometry for the given window handle.
--- row/col: 0-indexed terminal cell position of the window's top-left corner.
--- width/height: window dimensions in terminal cells.
function M.win_geometry(win)
    win = win or 0
    local pos = vim.api.nvim_win_get_position(win)
    return {
        row    = pos[1],
        col    = pos[2],
        width  = vim.api.nvim_win_get_width(win),
        height = vim.api.nvim_win_get_height(win),
    }
end

return M
