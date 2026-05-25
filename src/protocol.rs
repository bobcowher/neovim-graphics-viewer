use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Show { path: String, x: u32, y: u32, w: u32, h: u32 },
    Zoom { factor: f32 },
    Pan { dx: i32, dy: i32 },
    Reset,
    Quit,
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
        let json = r#"{"cmd":"show","path":"/tmp/a.png","x":0,"y":0,"w":80,"h":24}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Show { path, x, y, w, h } => {
                assert_eq!(path, "/tmp/a.png");
                assert_eq!(x, 0);
                assert_eq!(y, 0);
                assert_eq!(w, 80);
                assert_eq!(h, 24);
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
}
