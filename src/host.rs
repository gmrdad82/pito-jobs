pub(crate) fn host_name() -> String {
    let proc = std::fs::read("/proc/sys/kernel/hostname")
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    pick_host([
        proc,
        os_host_name(),
        std::env::var("HOSTNAME").ok(),
        std::env::var("COMPUTERNAME").ok(),
    ])
}

fn pick_host(candidates: impl IntoIterator<Item = Option<String>>) -> String {
    candidates
        .into_iter()
        .flatten()
        .map(|text| {
            let name: String = text.chars().filter(|c| !c.is_control()).collect();
            name.trim().to_string()
        })
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(unix)]
fn os_host_name() -> Option<String> {
    let mut buf = [0u8; 256];
    let status = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if status != 0 {
        return None;
    }
    let end = buf.iter().position(|byte| *byte == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

#[cfg(not(unix))]
fn os_host_name() -> Option<String> {
    None
}

pub(crate) fn pid_start() -> u64 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|text| pid_start_from_stat(&text))
        .unwrap_or(0)
}

fn pid_start_from_stat(text: &str) -> Option<u64> {
    let close = text.rfind(')')?;
    let after = text.get(close + 1..)?;
    after.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{pick_host, pid_start_from_stat};

    #[test]
    fn field_22_is_the_start_time() {
        let text = "1432 (prender-hd) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 98231 100 0\n";
        assert_eq!(pid_start_from_stat(text), Some(98231));
    }

    #[test]
    fn comm_may_contain_spaces_and_parentheses() {
        let text = "1432 (pre (nder) hd) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 98231 100\n";
        assert_eq!(pid_start_from_stat(text), Some(98231));
    }

    #[test]
    fn a_short_line_has_no_start_time() {
        assert_eq!(pid_start_from_stat("1432 (hd) S 1 1"), None);
        assert_eq!(pid_start_from_stat("no parentheses"), None);
    }

    #[test]
    fn the_host_falls_through_to_the_first_name_that_is_filled() {
        let text = |value: &str| Some(value.to_string());
        assert_eq!(pick_host([text("desk\n"), text("other")]), "desk");
        assert_eq!(pick_host([None, text("  \n"), text("macbook")]), "macbook");
        assert_eq!(pick_host([text("\u{7}"), None, text("winbox ")]), "winbox");
        assert_eq!(pick_host([None, text("")]), "unknown");
    }

    #[test]
    fn the_machine_names_itself_without_proc() {
        assert!(!super::host_name().is_empty());
        #[cfg(unix)]
        assert!(super::os_host_name().is_some_and(|name| !name.trim().is_empty()));
    }
}
