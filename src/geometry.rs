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
pub fn compute_geometry(x_cells: u32, y_cells: u32, w_cells: u32, h_cells: u32) -> Result<PixelGeometry, String> {
    let (cell_w, cell_h) = cell_pixel_size()?;
    let (term_x, term_y) = terminal_screen_position()?;
    Ok(PixelGeometry {
        x: term_x + (x_cells * cell_w) as i32,
        y: term_y + (y_cells * cell_h) as i32,
        width: (w_cells * cell_w).max(1),
        height: (h_cells * cell_h).max(1),
    })
}

/// Return (cell_pixel_width, cell_pixel_height) from the terminal.
pub fn cell_pixel_size() -> Result<(u32, u32), String> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        let ret = libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws);
        if ret != 0 {
            return Err("ioctl TIOCGWINSZ failed".into());
        }
        if ws.ws_col == 0 || ws.ws_row == 0 {
            return Err("terminal reports zero cell dimensions".into());
        }
        if ws.ws_xpixel == 0 || ws.ws_ypixel == 0 {
            return Err("terminal does not report pixel dimensions (try a different terminal)".into());
        }
        Ok((
            ws.ws_xpixel as u32 / ws.ws_col as u32,
            ws.ws_ypixel as u32 / ws.ws_row as u32,
        ))
    }
}

/// Return (x, y) screen position of the terminal window via X11.
pub fn terminal_screen_position() -> Result<(i32, i32), String> {
    let (conn, screen_num) = x11rb::connect(None)
        .map_err(|e| format!("X11 connect failed: {e}"))?;
    let root = conn.setup().roots[screen_num].root;
    let win_id = terminal_window_id(&conn, root)?;

    let reply = conn
        .translate_coordinates(win_id, root, 0, 0)
        .map_err(|e| format!("translate_coordinates request failed: {e}"))?
        .reply()
        .map_err(|e| format!("translate_coordinates reply failed: {e}"))?;

    Ok((reply.dst_x as i32, reply.dst_y as i32))
}

fn terminal_window_id(conn: &(impl Connection + x11rb::protocol::xproto::ConnectionExt), root: u32) -> Result<u32, String> {
    // Prefer $WINDOWID (set by most terminal emulators)
    if let Ok(s) = std::env::var("WINDOWID") {
        if let Ok(id) = s.parse::<u32>() {
            if id != 0 {
                return Ok(id);
            }
        }
    }

    // Fallback: _NET_ACTIVE_WINDOW from the root window
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

    // Tests for the pure arithmetic — no ioctl or X11 needed.

    fn make_geo(x_cells: u32, y_cells: u32, w_cells: u32, h_cells: u32,
                term_x: i32, term_y: i32, cell_w: u32, cell_h: u32) -> PixelGeometry {
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
