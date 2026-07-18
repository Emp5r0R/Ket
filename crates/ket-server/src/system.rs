use std::fs;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SystemSnapshot {
    pub cpu_load_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
}

pub(crate) fn snapshot() -> SystemSnapshot {
    let memory = memory();
    SystemSnapshot {
        cpu_load_percent: cpu_load_percent(),
        memory_used_bytes: memory.map(|(used, _)| used),
        memory_total_bytes: memory.map(|(_, total)| total),
        uptime_seconds: uptime_seconds(),
    }
}

fn cpu_load_percent() -> Option<f32> {
    let load_average = fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse::<f32>()
        .ok()?;
    let processors = std::thread::available_parallelism().ok()?.get() as f32;
    Some((load_average / processors * 100.0).clamp(0.0, 100.0))
}

fn memory() -> Option<(u64, u64)> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kib = None;
    let mut available_kib = None;
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        match fields.next()? {
            "MemTotal:" => total_kib = fields.next()?.parse::<u64>().ok(),
            "MemAvailable:" => available_kib = fields.next()?.parse::<u64>().ok(),
            _ => {}
        }
    }
    let total = total_kib?.saturating_mul(1024);
    let available = available_kib?.saturating_mul(1024);
    Some((total.saturating_sub(available), total))
}

fn uptime_seconds() -> Option<u64> {
    fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .map(|value| value as u64)
}
