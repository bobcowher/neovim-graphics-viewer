use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Show { path: String, x: u32, y: u32, w: u32, h: u32, cols: u32, rows: u32 },
    Zoom { factor: f32 },
    Pan { dx: i32, dy: i32 },
    Reset,
    Quit,
    PlayPause,
    Seek { delta: i32 },
    Rewind,
}

#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Ready,
    Error { msg: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_show() {
        let json = r#"{"cmd":"show","path":"/tmp/a.png","x":0,"y":0,"w":80,"h":24,"cols":200,"rows":50}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Show { path, x, y, w, h, cols, rows } => {
                assert_eq!(path, "/tmp/a.png");
                assert_eq!(x, 0);
                assert_eq!(y, 0);
                assert_eq!(w, 80);
                assert_eq!(h, 24);
                assert_eq!(cols, 200);
                assert_eq!(rows, 50);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserialize_zoom() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"zoom","factor":1.25}"#).unwrap();
        match cmd {
            Command::Zoom { factor } => assert!((factor - 1.25).abs() < 0.001),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserialize_pan() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"pan","dx":-1,"dy":0}"#).unwrap();
        match cmd {
            Command::Pan { dx, dy } => { assert_eq!(dx, -1); assert_eq!(dy, 0); }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserialize_reset() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"reset"}"#).unwrap();
        assert!(matches!(cmd, Command::Reset));
    }

    #[test]
    fn deserialize_quit() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"quit"}"#).unwrap();
        assert!(matches!(cmd, Command::Quit));
    }

    #[test]
    fn serialize_ready() {
        let json = serde_json::to_string(&Event::Ready).unwrap();
        assert_eq!(json, r#"{"event":"ready"}"#);
    }

    #[test]
    fn serialize_error() {
        let json = serde_json::to_string(&Event::Error { msg: "oops".into() }).unwrap();
        assert_eq!(json, r#"{"event":"error","msg":"oops"}"#);
    }

    #[test]
    fn deserialize_play_pause() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"play_pause"}"#).unwrap();
        assert!(matches!(cmd, Command::PlayPause));
    }

    #[test]
    fn deserialize_seek_forward() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"seek","delta":1}"#).unwrap();
        match cmd {
            Command::Seek { delta } => assert_eq!(delta, 1),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserialize_seek_backward() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"seek","delta":-10}"#).unwrap();
        match cmd {
            Command::Seek { delta } => assert_eq!(delta, -10),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserialize_rewind() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"rewind"}"#).unwrap();
        assert!(matches!(cmd, Command::Rewind));
    }
}
