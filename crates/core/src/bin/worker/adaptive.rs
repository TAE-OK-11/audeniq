//! Adaptive queue concurrency: a queue runs between `QUEUE_<Q>_CONCURRENCY`
//! (the floor) and `QUEUE_<Q>_MAX_CONCURRENCY` jobs at once. Every second a
//! controller looks at the backlog and at the worker's own CPU and
//! memory (its cgroup when it has one, so a container's `cpus`/`mem_limit`
//! count, not the host's), and moves the target:
//!
//! - up when jobs are waiting, every allowed loop is busy, and there is
//!   headroom (CPU < 75 %, memory < 70 %): two steps while the CPU is mostly
//!   idle (< 40 %), one step otherwise — a track takes seconds, so a slow
//!   ramp would finish a whole album before reaching its capacity;
//! - down at once under pressure (CPU ≥ 95 %, memory ≥ 85 %);
//! - back down to the floor after ~30 s with nothing waiting, so the next
//!   burst starts conservatively again.
//!
//! Lowering the target never interrupts a running job: loops above the
//! target just stop claiming new ones. CPU-bound media work is still capped
//! process-wide by the media slots (`AUDENIQ_QC_ASSET_PARALLELISM`), so
//! extra jobs on a small server overlap their download with another job's
//! decode instead of decoding side by side.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

pub(super) const TICK: Duration = Duration::from_secs(1);
const GROW_CPU: f64 = 0.75;
const GROW_MEM: f64 = 0.70;
/// Below this CPU share the target grows two steps at a time.
const FAST_CPU: f64 = 0.40;
const SHRINK_CPU: f64 = 0.95;
const SHRINK_MEM: f64 = 0.85;
/// Ticks without backlog (≈30 s) before the target steps back down.
const IDLE_TICKS: u32 = 30;

pub(super) struct Gate {
    pub min: usize,
    pub max: usize,
    target: AtomicUsize,
    active: AtomicUsize,
    /// Wakes loops that were above the target when it grows.
    pub grow: Notify,
}

impl Gate {
    pub fn new(min: usize, max: usize) -> Self {
        Self {
            min,
            max,
            target: AtomicUsize::new(min),
            active: AtomicUsize::new(0),
            grow: Notify::new(),
        }
    }
    pub fn target(&self) -> usize {
        self.target.load(Ordering::SeqCst)
    }
    pub fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }
    /// Counts one running job until dropped.
    pub fn running(&self) -> Running<'_> {
        self.active.fetch_add(1, Ordering::SeqCst);
        Running(self)
    }
    fn set(&self, target: usize) {
        let before = self.target.swap(target, Ordering::SeqCst);
        if target > before {
            self.grow.notify_waiters();
        }
    }
}

pub(super) struct Running<'a>(&'a Gate);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Load {
    /// Share of the CPU the worker may use (0..1+), `None` if unknown.
    pub cpu: Option<f64>,
    /// Share of the memory limit in use (without reclaimable file cache).
    pub mem: Option<f64>,
}

/// One controller step. `idle` counts consecutive ticks without backlog.
pub(super) fn decide(
    gate_min: usize,
    gate_max: usize,
    target: usize,
    active: usize,
    backlog: usize,
    load: Load,
    idle: &mut u32,
) -> usize {
    let cpu = load.cpu.unwrap_or(1.0);
    let mem = load.mem.unwrap_or(0.0);
    if cpu >= SHRINK_CPU || mem >= SHRINK_MEM {
        *idle = 0;
        return target.saturating_sub(1).max(gate_min);
    }
    if backlog > 0 {
        *idle = 0;
        // Unknown CPU (no counters) never grows past the floor.
        if active >= target && load.cpu.is_some() && cpu < GROW_CPU && mem < GROW_MEM {
            let step = if cpu < FAST_CPU { 2 } else { 1 };
            return (target + step)
                .min(backlog + active)
                .min(gate_max)
                .max(target);
        }
        return target;
    }
    *idle += 1;
    if *idle >= IDLE_TICKS && target > gate_min {
        *idle = 0;
        return target - 1;
    }
    target
}

/// CPU and memory of this process's cgroup (v2, then v1), falling back to
/// the whole machine (/proc).
pub(super) struct Probe {
    last: Option<(Instant, f64)>,
}

impl Probe {
    pub fn new() -> Self {
        Self { last: None }
    }

    pub fn sample(&mut self) -> Load {
        let now = Instant::now();
        let cpu = cpu_seconds().and_then(|(used, cpus)| {
            let previous = self.last.replace((now, used));
            let (at, before) = previous?;
            let wall = now.duration_since(at).as_secs_f64();
            (wall > 0.0 && cpus > 0.0).then(|| ((used - before) / wall / cpus).max(0.0))
        });
        Load {
            cpu,
            mem: memory_share(),
        }
    }
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn stat_field(text: &str, key: &str) -> Option<f64> {
    text.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        (it.next()? == key).then(|| it.next()?.parse().ok())?
    })
}

/// (CPU seconds used so far, CPUs available).
fn cpu_seconds() -> Option<(f64, f64)> {
    let host = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    // cgroup v2
    if let Some(stat) = read("/sys/fs/cgroup/cpu.stat") {
        let used = stat_field(&stat, "usage_usec")? / 1e6;
        let cpus = read("/sys/fs/cgroup/cpu.max")
            .and_then(|m| parse_cpu_max(&m))
            .unwrap_or(host);
        return Some((used, cpus));
    }
    // cgroup v1
    if let Some(ns) = read("/sys/fs/cgroup/cpuacct/cpuacct.usage")
        .or_else(|| read("/sys/fs/cgroup/cpu,cpuacct/cpuacct.usage"))
        .and_then(|s| s.trim().parse::<f64>().ok())
    {
        let quota = read("/sys/fs/cgroup/cpu/cpu.cfs_quota_us")
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|q| *q > 0.0);
        let period = read("/sys/fs/cgroup/cpu/cpu.cfs_period_us")
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|p| *p > 0.0);
        let cpus = quota.zip(period).map_or(host, |(q, p)| q / p);
        return Some((ns / 1e9, cpus));
    }
    // Whole machine: busy jiffies from /proc/stat (USER_HZ = 100).
    let stat = read("/proc/stat")?;
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<f64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    let idle = v.get(3)? + v.get(4).unwrap_or(&0.0);
    let busy = v.iter().take(8).sum::<f64>() - idle;
    Some((busy / 100.0, host))
}

/// `cpu.max` ("max 100000" = unlimited, "150000 100000" = 1.5 CPUs).
fn parse_cpu_max(text: &str) -> Option<f64> {
    let mut it = text.split_whitespace();
    let quota = it.next()?;
    let period: f64 = it.next()?.parse().ok()?;
    (quota != "max" && period > 0.0).then(|| quota.parse::<f64>().ok().map(|q| q / period))?
}

/// Memory in use (minus inactive file cache, like `docker stats`) over the
/// limit; the machine's MemAvailable when there is no cgroup limit.
fn memory_share() -> Option<f64> {
    let meminfo = read("/proc/meminfo");
    let total = meminfo
        .as_deref()
        .and_then(|m| stat_field(m, "MemTotal:"))
        .map(|kb| kb * 1024.0);
    let cgroup = read("/sys/fs/cgroup/memory.current")
        .map(|c| {
            (
                c,
                read("/sys/fs/cgroup/memory.max"),
                read("/sys/fs/cgroup/memory.stat"),
                "inactive_file",
            )
        })
        .or_else(|| {
            read("/sys/fs/cgroup/memory/memory.usage_in_bytes").map(|c| {
                (
                    c,
                    read("/sys/fs/cgroup/memory/memory.limit_in_bytes"),
                    read("/sys/fs/cgroup/memory/memory.stat"),
                    "total_inactive_file",
                )
            })
        });
    if let Some((current, limit, stat, inactive_key)) = cgroup {
        let current: f64 = current.trim().parse().ok()?;
        let inactive = stat
            .as_deref()
            .and_then(|s| stat_field(s, inactive_key))
            .unwrap_or(0.0);
        // "max", or v1's huge number, means no limit: use the machine's RAM.
        let limit = limit
            .and_then(|l| l.trim().parse::<f64>().ok())
            .filter(|l| total.is_none_or(|t| *l < t))
            .or(total)?;
        return Some(((current - inactive).max(0.0) / limit).min(2.0));
    }
    let m = meminfo?;
    let available = stat_field(&m, "MemAvailable:")? * 1024.0;
    Some(1.0 - available / total?)
}

/// Jobs ready to run in `queue` (capped: only "some or none" matters).
async fn backlog(pool: &sqlx::PgPool, queue: &str) -> usize {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM (SELECT 1 FROM operations.jobs WHERE queue=$1 AND status='QUEUED' AND attempts<max_attempts AND run_at<=clock_timestamp() LIMIT 32) ready",
    )
    .bind(queue)
    .fetch_one(pool)
    .await
    .map_or(0, |n| n as usize)
}

/// Runs forever: one step every [`TICK`].
pub(super) async fn control(pool: sqlx::PgPool, queue: &'static str, gate: std::sync::Arc<Gate>) {
    let mut probe = Probe::new();
    let mut idle = 0;
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let load = probe.sample();
        let (target, active) = (gate.target(), gate.active());
        // Ask the database only while every allowed loop is busy.
        let waiting = if active >= target {
            backlog(&pool, queue).await
        } else {
            0
        };
        let next = decide(gate.min, gate.max, target, active, waiting, load, &mut idle);
        if next != target {
            gate.set(next);
            tracing::info!(
                queue,
                from = target,
                to = next,
                active,
                waiting,
                cpu = load.cpu.map(|c| (c * 100.0).round()),
                mem = load.mem.map(|m| (m * 100.0).round()),
                "queue concurrency changed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALM: Load = Load {
        cpu: Some(0.3),
        mem: Some(0.3),
    };

    #[test]
    fn grows_one_step_while_jobs_wait_and_there_is_headroom() {
        let mut idle = 0;
        // Floor 1, max 4: busy with a backlog. A mostly idle CPU takes two
        // steps, a moderately busy one a single step.
        assert_eq!(decide(1, 4, 1, 1, 5, CALM, &mut idle), 3);
        let warm = Load {
            cpu: Some(0.6),
            mem: Some(0.3),
        };
        assert_eq!(decide(1, 4, 3, 3, 5, warm, &mut idle), 4);
        assert_eq!(decide(1, 4, 4, 4, 5, CALM, &mut idle), 4, "never above max");
        // Never more loops than there are jobs to run.
        assert_eq!(decide(1, 4, 1, 1, 1, CALM, &mut idle), 2);
        // A free loop still exists: no need to grow yet.
        assert_eq!(decide(1, 4, 2, 1, 5, CALM, &mut idle), 2);
    }

    #[test]
    fn no_growth_without_headroom_or_without_cpu_counters() {
        let mut idle = 0;
        let busy_cpu = Load {
            cpu: Some(0.8),
            mem: Some(0.3),
        };
        let full_mem = Load {
            cpu: Some(0.3),
            mem: Some(0.75),
        };
        let unknown = Load {
            cpu: None,
            mem: Some(0.3),
        };
        assert_eq!(decide(1, 3, 1, 1, 5, busy_cpu, &mut idle), 1);
        assert_eq!(
            decide(
                1,
                3,
                2,
                2,
                5,
                Load {
                    cpu: Some(0.7),
                    mem: Some(0.3)
                },
                &mut idle
            ),
            3
        );
        assert_eq!(decide(1, 3, 1, 1, 5, full_mem, &mut idle), 1);
        assert_eq!(decide(1, 3, 1, 1, 5, unknown, &mut idle), 1);
    }

    #[test]
    fn shrinks_at_once_under_pressure_but_not_below_the_floor() {
        let mut idle = 0;
        let hot = Load {
            cpu: Some(0.97),
            mem: Some(0.3),
        };
        let tight = Load {
            cpu: Some(0.2),
            mem: Some(0.9),
        };
        assert_eq!(decide(1, 4, 3, 3, 5, hot, &mut idle), 2);
        assert_eq!(decide(1, 4, 2, 2, 5, tight, &mut idle), 1);
        assert_eq!(decide(1, 4, 1, 1, 5, tight, &mut idle), 1);
    }

    #[test]
    fn returns_to_the_floor_after_a_quiet_spell() {
        let mut idle = 0;
        let mut target = 3;
        for _ in 0..IDLE_TICKS - 1 {
            target = decide(1, 3, target, 0, 0, CALM, &mut idle);
        }
        assert_eq!(target, 3, "a short pause keeps the capacity");
        target = decide(1, 3, target, 0, 0, CALM, &mut idle);
        assert_eq!(target, 2);
        // Work arriving resets the quiet spell.
        target = decide(1, 3, target, 2, 4, CALM, &mut idle);
        assert_eq!((target, idle), (3, 0));
    }

    #[test]
    fn parses_cgroup_cpu_limits() {
        assert_eq!(parse_cpu_max("max 100000\n"), None);
        assert_eq!(parse_cpu_max("150000 100000\n"), Some(1.5));
        assert_eq!(parse_cpu_max("100000 100000"), Some(1.0));
    }

    #[test]
    fn running_guard_counts_jobs() {
        let gate = Gate::new(1, 3);
        {
            let _a = gate.running();
            let _b = gate.running();
            assert_eq!(gate.active(), 2);
        }
        assert_eq!(gate.active(), 0);
        gate.set(2);
        assert_eq!(gate.target(), 2);
    }

    #[test]
    fn probe_reads_this_machine() {
        let mut p = Probe::new();
        let _ = p.sample();
        std::thread::sleep(Duration::from_millis(50));
        let load = p.sample();
        assert!(load.cpu.is_some_and(|c| c >= 0.0), "{load:?}");
        assert!(
            load.mem.is_some_and(|m| (0.0..=2.0).contains(&m)),
            "{load:?}"
        );
    }
}
