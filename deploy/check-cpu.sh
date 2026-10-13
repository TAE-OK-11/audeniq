#!/usr/bin/env bash
# Check before executing any AUDENIQ binary. Optional host/cpuinfo arguments
# allow testing real deployment decisions without emulating a CPU.
set -euo pipefail
image_arch=${1:?usage: check-cpu.sh IMAGE_ARCH TARGET_CPU [HOST_ARCH CPUINFO]}
target_cpu=${2:?missing target CPU}
host_arch=${3:-$(uname -m)}
cpuinfo=${4:-/proc/cpuinfo}
die() { echo "deploy: $*" >&2; exit 1; }
case "$host_arch" in
  x86_64|amd64) host_arch=amd64 ;;
  aarch64|arm64) host_arch=arm64 ;;
  *) die "지원하지 않는 서버 아키텍처: $host_arch" ;;
esac
[ "$image_arch" = "$host_arch" ] || die "이미지는 $image_arch, 서버는 $host_arch 입니다. 서버 아키텍처에 맞는 이미지를 사용해 주세요"

case "$image_arch:$target_cpu" in
  amd64:x86-64|arm64:generic) exit 0 ;;
  amd64:x86-64-v2) need="cx16 lahf_lm popcnt pni sse4_1 sse4_2 ssse3"; field=flags ;;
  amd64:x86-64-v3) need="cx16 lahf_lm popcnt pni sse4_1 sse4_2 ssse3 avx avx2 bmi1 bmi2 f16c fma abm movbe xsave"; field=flags ;;
  arm64:neoverse-v2)
    # Arm's Neoverse V2 MIDR: implementer 0x41, part 0xd4f. Checking every
    # exposed core also refuses heterogeneous hosts and older Gravitons.
    # SVE is kernel-managed, so require its advertised availability too.
    awk -F: '
      /CPU implementer/ { n++; value=$2; gsub(/[[:space:]]/, "", value); if (tolower(value) != "0x41") bad=1 }
      /CPU part/ { p++; value=$2; gsub(/[[:space:]]/, "", value); if (tolower(value) != "0xd4f") bad=1 }
      END { exit (n == 0 || n != p || bad) }
    ' "$cpuinfo" || die "neoverse-v2 이미지는 Graviton4/Neoverse V2 서버가 필요해요. 다른 ARM 서버에는 공용 이미지를 사용해 주세요"
    need="fp asimd atomics crc32 sve sve2 i8mm bf16"; field=Features
    ;;
  arm64:neoverse-n1)
    # Oracle Cloud Ampere A1 (Ampere Altra = Neoverse N1), also AWS Graviton2.
    # N1 code (ARMv8.2 + dot product + RCpc) runs on any newer core that has
    # these features (Graviton3/4, AmpereOne), so only the features are checked.
    need="fp asimd atomics crc32 asimddp lrcpc"; field=Features
    ;;
  *) die "알 수 없는 CPU 기준선: $image_arch:$target_cpu" ;;
esac

# Use all online processors, rather than trusting the first core's flags.
awk -F: -v field="$field" -v need="$need" '
  $1 ~ "^[[:space:]]*" field "[[:space:]]*$" {
    seen++
    flags=$2
    gsub(/[[:space:]]+/, " ", flags)
    split(need, required, " ")
    for (i in required) {
      if (index(" " flags " ", " " required[i] " ") == 0) {
        printf "missing CPU feature: %s\n", required[i] > "/dev/stderr"
        bad=1
      }
    }
  }
  END { exit (seen == 0 || bad) }
' "$cpuinfo" || die "이 서버 CPU는 이미지 기준선 $target_cpu 를 지원하지 않아요. 공용 이미지를 사용해 주세요"
