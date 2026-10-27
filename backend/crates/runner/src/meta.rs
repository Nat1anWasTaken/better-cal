//! Parser for isolate's `--meta` file (`key:value` per line).

use std::collections::HashMap;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Meta {
    pub status: Option<String>,
    pub message: Option<String>,
    pub exitcode: Option<i32>,
    pub exitsig: Option<i32>,
    pub time_s: f64,
    pub wall_time_s: f64,
    pub max_rss_kb: u32,
    pub cg_mem_kb: u32,
    pub cg_oom_killed: bool,
    pub killed: bool,
}

impl Meta {
    pub fn parse(text: &str) -> Meta {
        let kv: HashMap<&str, &str> = text.lines().filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim(), v.trim())).collect();
        let num = |k: &str| kv.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        Meta {
            status: kv.get("status").map(|s| s.to_string()),
            message: kv.get("message").map(|s| s.to_string()),
            exitcode: kv.get("exitcode").and_then(|v| v.parse().ok()),
            exitsig: kv.get("exitsig").and_then(|v| v.parse().ok()),
            time_s: num("time"),
            wall_time_s: num("time-wall"),
            max_rss_kb: num("max-rss") as u32,
            cg_mem_kb: num("cg-mem") as u32,
            cg_oom_killed: kv.contains_key("cg-oom-killed"),
            killed: kv.contains_key("killed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signal_and_oom() {
        let m = Meta::parse("time:0.037\ntime-wall:0.040\nmax-rss:66048\ncg-mem:65536\ncg-oom-killed:1\nexitsig:9\nstatus:SG\nmessage:Caught fatal signal 9\n");
        assert_eq!(m.status.as_deref(), Some("SG"));
        assert_eq!(m.exitsig, Some(9));
        assert!(m.cg_oom_killed);
        assert_eq!(m.max_rss_kb, 66048);
        assert!((m.time_s - 0.037).abs() < 1e-9);
    }

    #[test]
    fn parses_normal_exit() {
        let m = Meta::parse("time:0.001\ntime-wall:0.001\nmax-rss:1288\nexitcode:0\n");
        assert_eq!(m.status, None);
        assert_eq!(m.exitcode, Some(0));
        assert!(!m.cg_oom_killed);
    }
}
