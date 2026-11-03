//! Session selection and native desktop connections shared by the frontends.
pub mod clipboard;
pub mod x11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    X11,
    Wayland,
}

impl Session {
    pub fn current() -> Result<Self, String> {
        Self::detect(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()),
            std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty()),
        )
    }

    fn detect(kind: Option<&str>, wayland: bool, x11: bool) -> Result<Self, String> {
        match kind {
            Some("x11") if x11 => Ok(Self::X11),
            Some("wayland") if wayland => Ok(Self::Wayland),
            Some("x11" | "wayland") => {
                Err("선택한 데스크톱 세션의 연결 환경 변수가 없습니다".into())
            }
            _ if wayland => Ok(Self::Wayland),
            _ if x11 => Ok(Self::X11),
            _ => Err("X11 또는 Wayland 데스크톱 세션이 필요합니다".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    #[test]
    fn session_type_wins_over_stale_or_xwayland_display() {
        assert_eq!(Session::detect(Some("x11"), true, true), Ok(Session::X11));
        assert_eq!(
            Session::detect(Some("wayland"), true, true),
            Ok(Session::Wayland)
        );
        assert!(Session::detect(Some("wayland"), false, true).is_err());
        assert_eq!(Session::detect(None, false, true), Ok(Session::X11));
        assert!(Session::detect(None, false, false).is_err());
    }
}
