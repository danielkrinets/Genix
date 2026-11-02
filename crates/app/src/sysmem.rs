//! Installed memory of the computer, to pick lighter graphics on small machines.

/// Total RAM in GiB, if the system says.
pub fn total_gib() -> Option<f64> {
    let bytes: u64 = if cfg!(target_os = "macos") {
        let out = std::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()?
    } else if cfg!(target_os = "linux") {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = text.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
        kb * 1024
    } else if cfg!(target_os = "windows") {
        let out = std::process::Command::new("wmic").args(["ComputerSystem", "get", "TotalPhysicalMemory"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).lines().find_map(|l| l.trim().parse::<u64>().ok())?
    } else {
        return None;
    };
    Some(bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// Samples per pixel for "Auto": none on 4 GiB and below, two on 8 GiB and below, four above (or when unknown).
pub fn auto_msaa(gib: Option<f64>) -> u32 {
    match gib {
        Some(g) if g <= 4.5 => 1,
        Some(g) if g <= 8.5 => 2,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn small_machines_get_lighter_anti_aliasing() {
        assert_eq!(super::auto_msaa(Some(4.0)), 1);
        assert_eq!(super::auto_msaa(Some(8.0)), 2);
        assert_eq!(super::auto_msaa(Some(16.0)), 4);
        assert_eq!(super::auto_msaa(None), 4);
    }
}
