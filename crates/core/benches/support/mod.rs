use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
    time::Instant,
};

struct CountingAllocator;
static TRACK: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(size: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    let live = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(live, Relaxed);
}

// SAFETY: all allocation operations, including their original layouts, are
// forwarded to System. Tracking only records sizes; it never touches pointers.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && TRACK.load(Relaxed) {
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() && TRACK.load(Relaxed) {
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRACK.load(Relaxed) {
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, size) };
        if !new.is_null() && TRACK.load(Relaxed) {
            LIVE.fetch_sub(layout.size(), Relaxed);
            allocated(size);
        }
        new
    }
}
#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

pub(crate) fn allocations(mut f: impl FnMut()) -> (usize, usize, usize) {
    ALLOCS.store(0, Relaxed);
    LIVE.store(0, Relaxed);
    PEAK.store(0, Relaxed);
    TRACK.store(true, Relaxed);
    f();
    TRACK.store(false, Relaxed);
    (ALLOCS.load(Relaxed), PEAK.load(Relaxed), LIVE.load(Relaxed))
}

fn cpu_seconds() -> f64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the provided rusage on success.
    assert_eq!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) },
        0
    );
    let usage = unsafe { usage.assume_init() };
    (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as f64
        + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1e6
}

fn timed(f: &mut impl FnMut(), n: usize) -> (f64, f64) {
    let start = Instant::now();
    let cpu = cpu_seconds();
    for _ in 0..n {
        f();
    }
    (
        (cpu_seconds() - cpu) * 1e6 / n as f64,
        start.elapsed().as_secs_f64() * 1e6 / n as f64,
    )
}

pub(crate) fn compare(name: &str, n: usize, mut old: impl FnMut(), mut new: impl FnMut()) {
    // Warm both paths before measuring transient allocations or timings.
    for _ in 0..5 {
        old();
        new();
    }
    let a = allocations(&mut old);
    let b = allocations(&mut new);
    assert_eq!(a.2, 0, "reference must release transient allocations");
    assert_eq!(b.2, 0, "optimized must release transient allocations");
    let mut old_times = Vec::new();
    let mut new_times = Vec::new();
    for round in 0..5 {
        let (a, b) = if round % 2 == 0 {
            (timed(&mut old, n), timed(&mut new, n))
        } else {
            let b = timed(&mut new, n);
            (timed(&mut old, n), b)
        };
        old_times.push(a);
        new_times.push(b);
    }
    old_times.sort_by(|a, b| a.0.total_cmp(&b.0));
    new_times.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!(
        "{name}: n={n} CPU_us={:.3}->{:.3} wall_us={:.3}->{:.3} allocations={}->{} peak_payload_bytes={}->{}",
        old_times[2].0, new_times[2].0, old_times[2].1, new_times[2].1, a.0, b.0, a.1, b.1
    );
}
