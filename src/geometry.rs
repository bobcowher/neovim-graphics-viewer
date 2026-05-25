use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

#[derive(Debug, Clone)]
pub struct PixelGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Convert cell-space geometry to screen pixel geometry.
///
/// `total_cols`/`total_rows` are the full terminal dimensions (vim.o.columns / vim.o.lines).
/// Cell pixel size is derived by dividing the terminal window's X11 pixel geometry by those
/// totals. This avoids TIOCGWINSZ entirely, which is unavailable when launched via jobstart()
/// because Neovim calls setsid() and the child has no controlling terminal.
pub fn compute_geometry(
    x_cells: u32,
    y_cells: u32,
    w_cells: u32,
    h_cells: u32,
    total_cols: u32,
    total_rows: u32,
) -> Result<PixelGeometry, String> {
    if total_cols == 0 || total_rows == 0 {
        return Err("terminal reports zero dimensions".into());
    }

    let (conn, screen_num) = x11rb::connect(None)
        .map_err(|e| format!("X11 connect failed: {e}"))?;
    let root = conn.setup().roots[screen_num].root;
    let win_id = terminal_window_id(&conn, root)?;

    let geom = conn
        .get_geometry(win_id)
        .map_err(|e| format!("get_geometry request failed: {e}"))?
        .reply()
        .map_err(|e| format!("get_geometry reply failed: {e}"))?;

    let cell_w = geom.width as u32 / total_cols;
    let cell_h = geom.height as u32 / total_rows;

    if cell_w == 0 || cell_h == 0 {
        return Err(format!(
            "degenerate cell size {cell_w}x{cell_h} \
             (terminal {}x{} px, {total_cols}x{total_rows} cells)",
            geom.width, geom.height
        ));
    }

    let pos = conn
        .translate_coordinates(win_id, root, 0, 0)
        .map_err(|e| format!("translate_coordinates request failed: {e}"))?
        .reply()
        .map_err(|e| format!("translate_coordinates reply failed: {e}"))?;

    Ok(PixelGeometry {
        x: pos.dst_x as i32 + (x_cells * cell_w) as i32,
        y: pos.dst_y as i32 + (y_cells * cell_h) as i32,
        width: (w_cells * cell_w).max(1),
        height: (h_cells * cell_h).max(1),
    })
}

fn terminal_window_id(
    conn: &(impl Connection + x11rb::protocol::xproto::ConnectionExt),
    root: u32,
) -> Result<u32, String> {
    if let Ok(s) = std::env::var("WINDOWID") {
        if let Ok(id) = s.parse::<u32>() {
            if id != 0 {
                return Ok(id);
            }
        }
    }

    let atom_reply = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|e| format!("intern_atom failed: {e}"))?
        .reply()
        .map_err(|e| format!("intern_atom reply failed: {e}"))?;

    let prop = conn
        .get_property(false, root, atom_reply.atom, AtomEnum::WINDOW, 0, 1)
        .map_err(|e| format!("get_property failed: {e}"))?
        .reply()
        .map_err(|e| format!("get_property reply failed: {e}"))?;

    if prop.value.len() >= 4 {
        let id = u32::from_ne_bytes(prop.value[..4].try_into().unwrap());
        if id != 0 {
            return Ok(id);
        }
    }

    Err("cannot determine terminal window ID — set $WINDOWID or use a compatible terminal".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_geo(
        x_cells: u32,
        y_cells: u32,
        w_cells: u32,
        h_cells: u32,
        term_x: i32,
        term_y: i32,
        cell_w: u32,
        cell_h: u32,
    ) -> PixelGeometry {
        PixelGeometry {
            x: term_x + (x_cells * cell_w) as i32,
            y: term_y + (y_cells * cell_h) as i32,
            width: (w_cells * cell_w).max(1),
            height: (h_cells * cell_h).max(1),
        }
    }

    #[test]
    fn geometry_math_origin() {
        let g = make_geo(0, 0, 80, 24, 100, 200, 8, 16);
        assert_eq!(g.x, 100);
        assert_eq!(g.y, 200);
        assert_eq!(g.width, 640);
        assert_eq!(g.height, 384);
    }

    #[test]
    fn geometry_math_offset() {
        let g = make_geo(5, 3, 70, 20, 0, 0, 10, 20);
        assert_eq!(g.x, 50);
        assert_eq!(g.y, 60);
        assert_eq!(g.width, 700);
        assert_eq!(g.height, 400);
    }

    #[test]
    fn geometry_zero_cells_gives_min_one() {
        let g = make_geo(0, 0, 0, 0, 0, 0, 8, 16);
        assert_eq!(g.width, 1);
        assert_eq!(g.height, 1);
    }
}
