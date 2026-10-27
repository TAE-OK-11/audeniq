// cargo bench --locked -p audeniq-core --bench edge_resources
// CloudFront 오리진 확인(crate::edge_tls)의 비용: 연결마다 하는 IP 조회, 6시간마다 하는 AWS 목록 해석.
// reference는 바꾸기 전 구현(대역 목록 선형 탐색, 항목마다 String을 만드는 해석)이다.
// 입력은 지금 ip-ranges.json과 같은 모양·크기(2.7 MB, IPv4 10,688·IPv6 7,005개)로 만든다.
use audeniq_core::edge_tls::{Ranges, parse_aws_ranges};
use ipnet::IpNet;
use std::hint::black_box;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};
mod support;
use support::{allocations, compare};

const SERVICE: &str = "CLOUDFRONT_ORIGIN_FACING";

mod reference {
    use super::*;

    #[derive(serde::Deserialize)]
    struct AwsRanges {
        prefixes: Vec<AwsV4>,
        ipv6_prefixes: Vec<AwsV6>,
    }
    #[derive(serde::Deserialize)]
    struct AwsV4 {
        ip_prefix: IpNet,
        service: String,
    }
    #[derive(serde::Deserialize)]
    struct AwsV6 {
        ipv6_prefix: IpNet,
        service: String,
    }

    pub fn parse(text: &[u8]) -> Vec<IpNet> {
        let ranges: AwsRanges = serde_json::from_slice(text).unwrap();
        let v4 = ranges
            .prefixes
            .into_iter()
            .filter(|p| p.service == SERVICE)
            .map(|p| p.ip_prefix);
        let v6 = ranges
            .ipv6_prefixes
            .into_iter()
            .filter(|p| p.service == SERVICE)
            .map(|p| p.ipv6_prefix);
        let mut nets: Vec<IpNet> = v4.chain(v6).collect();
        nets.sort();
        nets.dedup();
        nets
    }

    pub struct Allow(pub RwLock<Arc<Vec<IpNet>>>);
    impl Allow {
        pub fn permits(&self, ip: IpAddr) -> bool {
            let ip = ip.to_canonical();
            self.0.read().unwrap().iter().any(|n| n.contains(&ip))
        }
    }
}

/// 실제 CloudFront 대역(저장소 스냅샷) + 다른 AWS 서비스 항목으로 채운 ip-ranges.json
fn aws_json() -> (Vec<u8>, Vec<IpNet>) {
    #[derive(serde::Deserialize)]
    struct Snapshot {
        prefixes: Vec<IpNet>,
    }
    let snapshot: Snapshot = serde_json::from_str(include_str!(
        "../../../config/cloudfront-origin-facing.json"
    ))
    .unwrap();
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for net in &snapshot.prefixes {
        let entry = |key: &str| serde_json::json!({key: net.to_string(), "region": "GLOBAL", "service": SERVICE, "network_border_group": "GLOBAL"});
        match net {
            IpNet::V4(_) => v4.push(entry("ip_prefix")),
            IpNet::V6(_) => v6.push(entry("ipv6_prefix")),
        }
    }
    let services = ["AMAZON", "EC2", "S3", "ROUTE53_HEALTHCHECKS", "CLOUDFRONT"];
    for i in v4.len()..10_688 {
        v4.push(serde_json::json!({"ip_prefix": format!("52.{}.{}.0/24", i / 256 % 256, i % 256), "region": "ap-northeast-2", "service": services[i % 5], "network_border_group": "ap-northeast-2"}));
    }
    for i in v6.len()..7_005 {
        v6.push(serde_json::json!({"ipv6_prefix": format!("2406:da{:02x}:{:x}::/56", i % 256, i), "region": "ap-northeast-2", "service": services[i % 5], "network_border_group": "ap-northeast-2"}));
    }
    let json = serde_json::json!({"syncToken": "1793100425", "createDate": "2026-10-27-11-27-05", "prefixes": v4, "ipv6_prefixes": v6});
    (serde_json::to_vec_pretty(&json).unwrap(), snapshot.prefixes)
}

fn main() {
    // 실제 파일로도 맞춰 보기: AUDENIQ_IP_RANGES_JSON=ip-ranges.json (curl https://ip-ranges.amazonaws.com/ip-ranges.json)
    if let Ok(path) = std::env::var("AUDENIQ_IP_RANGES_JSON") {
        let real = std::fs::read(path).unwrap();
        let old = reference::parse(&real);
        assert_eq!(parse_aws_ranges(&real).unwrap(), Ranges::from_nets(&old));
        println!(
            "real ip-ranges.json: {} bytes, {} CloudFront prefixes, same result",
            real.len(),
            old.len()
        );
    }
    let (json, cloudfront) = aws_json();
    println!("ip-ranges.json: {} bytes", json.len());
    let old = reference::parse(&json);
    let new = parse_aws_ranges(&json).unwrap();
    assert_eq!(new, Ranges::from_nets(&old), "same CloudFront ranges");
    println!(
        "ranges: prefixes={} merged_intervals(v4,v6)={:?} retained_bytes={}->{}",
        old.len(),
        new.intervals(),
        old.capacity() * std::mem::size_of::<IpNet>(),
        new.intervals().0 * 8 + new.intervals().1 * 32
    );
    compare(
        "parse ip-ranges.json",
        20,
        || drop(black_box(reference::parse(black_box(&json)))),
        || drop(black_box(parse_aws_ranges(black_box(&json)).unwrap())),
    );

    // 연결마다: CloudFront 대역 안(앞·뒤 대역), 밖(IPv4·IPv6)
    let old_allow = reference::Allow(RwLock::new(Arc::new(old)));
    let new_allow = RwLock::new(new);
    let first = cloudfront[0].addr();
    let last = cloudfront
        .iter()
        .rev()
        .find(|n| n.addr().is_ipv4())
        .unwrap()
        .addr();
    let probes: [(&str, IpAddr); 4] = [
        ("inside first", first),
        ("inside last", last),
        ("outside v4", "8.8.8.8".parse().unwrap()),
        ("outside v6", "2001:db8::1".parse().unwrap()),
    ];
    for (label, ip) in probes {
        assert_eq!(
            old_allow.permits(ip),
            new_allow.read().unwrap().contains(ip),
            "{label}"
        );
        compare(
            &format!("lookup {label} x1000"),
            2_000,
            || {
                for _ in 0..1000 {
                    black_box(old_allow.permits(black_box(ip)));
                }
            },
            || {
                for _ in 0..1000 {
                    black_box(new_allow.read().unwrap().contains(black_box(ip)));
                }
            },
        );
    }
    let swap = allocations(|| {
        *new_allow.write().unwrap() = parse_aws_ranges(&json).unwrap();
    });
    println!(
        "refresh (parse + swap): allocations={} peak_payload_bytes={} retained_delta_bytes={}",
        swap.0, swap.1, swap.2
    );
}
