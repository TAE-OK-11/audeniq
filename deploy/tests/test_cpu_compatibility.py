"""Exercise build flags and deployment gates using representative CPU records."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


DEPLOY = Path(__file__).resolve().parents[1]
X86_V3 = "cx16 lahf_lm popcnt pni sse4_1 sse4_2 ssse3 avx avx2 bmi1 bmi2 f16c fma abm movbe xsave"
ARM_V2 = "fp asimd atomics crc32 sve sve2 i8mm bf16"
# Ampere Altra (Oracle Cloud A1) as the kernel reports it
ARM_N1 = "fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics fphp asimdhp cpuid asimdrdm lrcpc dcpop asimddp ssbs"
# AmpereOne M (Oracle Cloud A4) as an ARMv8.6 kernel reports it (no MTE exposed in the VM)
ARM_A4 = ("fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics fphp asimdhp cpuid asimdrdm jscvt fcma lrcpc dcpop "
          "sha3 sm3 sm4 asimddp sha512 asimdfhm dit uscat ilrcpc flagm ssbs sb paca pacg dcpodp flagm2 frint i8mm bf16 rng bti ecv")


def arm_record(part="0xd4f", features=ARM_V2):
    return f"processor : 0\nCPU implementer : 0x41\nCPU part : {part}\nFeatures : {features}\n"


class CpuCompatibilityTests(unittest.TestCase):
    def check_cpu(self, arch, cpu, host, records="", succeeds=True):
        with tempfile.TemporaryDirectory() as directory:
            cpuinfo = Path(directory) / "cpuinfo"
            cpuinfo.write_text(records)
            result = subprocess.run(
                ["bash", str(DEPLOY / "check-cpu.sh"), arch, cpu, host, str(cpuinfo)],
                capture_output=True, text=True, check=False,
            )
        self.assertEqual(result.returncode == 0, succeeds, result.stderr)
        return result

    def test_portable_images_accept_both_architectures(self):
        self.check_cpu("amd64", "x86-64", "x86_64")
        self.check_cpu("arm64", "generic", "aarch64")

    def test_architecture_mismatch_is_rejected_even_with_emulation(self):
        self.check_cpu("amd64", "x86-64", "aarch64", succeeds=False)
        self.check_cpu("arm64", "generic", "x86_64", succeeds=False)

    def test_unknown_and_wrong_architecture_cpus_are_rejected(self):
        for arch, cpu, host in [
            ("amd64", "native", "x86_64"),
            ("arm64", "x86-64-v3", "aarch64"),
            ("amd64", "neoverse-v2", "x86_64"),
            ("arm64", "unknown", "aarch64"),
        ]:
            with self.subTest(arch=arch, cpu=cpu):
                self.check_cpu(arch, cpu, host, succeeds=False)

    def test_x86_v2_and_v3_features(self):
        self.check_cpu("amd64", "x86-64-v2", "x86_64", "flags : " + X86_V3)
        self.check_cpu("amd64", "x86-64-v3", "x86_64", "flags : " + X86_V3)
        self.check_cpu("amd64", "x86-64-v3", "x86_64", "flags : " + X86_V3.replace("avx2", ""), succeeds=False)
        self.check_cpu("amd64", "x86-64-v2", "x86_64", "flags : " + X86_V3.replace("pni", ""), succeeds=False)

    def test_x86_checks_every_core(self):
        self.check_cpu("amd64", "x86-64-v3", "x86_64", "flags : " + X86_V3 + "\nflags : sse2\n", succeeds=False)

    def test_graviton4_accepts_neoverse_v2(self):
        self.check_cpu("arm64", "neoverse-v2", "aarch64", arm_record() * 2)
        self.check_cpu("arm64", "neoverse-v2", "aarch64", arm_record(features=ARM_V2.replace(" ", "\t")))

    def test_graviton2_graviton3_and_other_arm_cpus_are_rejected(self):
        for part in ["0xd0c", "0xd40", "0xd49"]:
            self.check_cpu("arm64", "neoverse-v2", "aarch64", arm_record(part), succeeds=False)

    def test_graviton4_requires_kernel_sve2_and_lse_support(self):
        for feature in ["sve", "sve2", "atomics", "i8mm", "bf16"]:
            self.check_cpu("arm64", "neoverse-v2", "aarch64", arm_record(features=" ".join(f for f in ARM_V2.split() if f != feature)), succeeds=False)

    def test_ampere_a1_accepts_neoverse_n1_and_newer_cores(self):
        # Ampere Altra / Graviton2 (N1), Graviton3/4 and AmpereOne all have the N1 feature set.
        self.check_cpu("arm64", "neoverse-n1", "aarch64", arm_record("0xd0c", ARM_N1) * 4)
        self.check_cpu("arm64", "neoverse-n1", "aarch64", arm_record("0xd4f", ARM_N1 + " sve sve2"))
        self.check_cpu("arm64", "neoverse-n1", "aarch64", arm_record("0xac3", ARM_N1).replace("0x41", "0xc0"))

    def test_ampere_a1_rejects_cores_without_dot_product_or_lse(self):
        # Graviton1 / Cortex-A72 class (ARMv8.0): no atomics, no asimddp.
        self.check_cpu("arm64", "neoverse-n1", "aarch64", arm_record("0xd08", "fp asimd evtstrm aes pmull sha1 sha2 crc32 cpuid"), succeeds=False)
        for feature in ["atomics", "asimddp", "lrcpc"]:
            self.check_cpu("arm64", "neoverse-n1", "aarch64", arm_record("0xd0c", " ".join(f for f in ARM_N1.split() if f != feature)), succeeds=False)
        # One weaker core is enough to refuse.
        self.check_cpu("arm64", "neoverse-n1", "aarch64", arm_record("0xd0c", ARM_N1) + arm_record("0xd08", "fp asimd crc32"), succeeds=False)
        self.check_cpu("amd64", "neoverse-n1", "x86_64", succeeds=False)

    def test_oracle_a4_accepts_ampereone_and_graviton3_class_cores(self):
        self.check_cpu("arm64", "ampere1a", "aarch64", arm_record("0xac4", ARM_A4).replace("0x41", "0xc0") * 4)
        # Graviton4 (V2) reports the same v8.6 set plus SVE.
        self.check_cpu("arm64", "ampere1a", "aarch64", arm_record("0xd4f", ARM_A4 + " sve sve2"))

    def test_oracle_a4_rejects_altra_and_cores_missing_v86_features(self):
        # Ampere Altra / Oracle A1 (N1): no BF16, I8MM, FRINT, SHA3 ...
        self.check_cpu("arm64", "ampere1a", "aarch64", arm_record("0xd0c", ARM_N1), succeeds=False)
        for feature in ["bf16", "i8mm", "frint", "sha3", "ilrcpc", "atomics", "asimddp"]:
            self.check_cpu("arm64", "ampere1a", "aarch64", arm_record("0xac4", " ".join(f for f in ARM_A4.split() if f != feature)), succeeds=False)
        self.check_cpu("amd64", "ampere1a", "x86_64", succeeds=False)

    def test_arm_checks_all_cores_and_requires_cpu_identity(self):
        for records in ["", "Features : " + ARM_V2, arm_record() + arm_record("0xd40"), arm_record().replace("CPU implementer : 0x41", "CPU implementer : 0x42")]:
            self.check_cpu("arm64", "neoverse-v2", "aarch64", records, succeeds=False)

    def test_build_flags_apply_to_rust_and_native_dependencies(self):
        for arch, cpu, expected_cpu, native_flag in [
            ("amd64", "", "x86-64", "-march=x86-64"),
            ("amd64", "x86-64-v3", "x86-64-v3", "-march=x86-64-v3"),
            ("arm64", "", "generic", "-march=armv8-a"),
            ("arm64", "neoverse-v2", "neoverse-v2", "-mcpu=neoverse-v2"),
            ("arm64", "neoverse-n1", "neoverse-n1", "-mcpu=neoverse-n1"),
            ("arm64", "ampere1a", "ampere1a", "-mcpu=ampere1a"),
        ]:
            with self.subTest(arch=arch, cpu=cpu), tempfile.TemporaryDirectory() as directory:
                environment = dict(os.environ, RUSTFLAGS="-C debuginfo=0", CFLAGS="-fPIC", CXXFLAGS="-fPIC", AUDENIQ_BUILD_METADATA_DIR=directory)
                result = subprocess.run(
                    ["sh", str(DEPLOY / "build-target.sh"), arch, cpu, "python3", "-c", "import os,json; print(json.dumps({k:os.environ[k] for k in ['RUSTFLAGS','CFLAGS','CXXFLAGS','CARGO_BUILD_TARGET']}))"],
                    env=environment, capture_output=True, text=True, check=True,
                )
                flags = json.loads(result.stdout)
                self.assertEqual(flags["RUSTFLAGS"], f"-C debuginfo=0 -C target-cpu={expected_cpu}")
                self.assertEqual(flags["CFLAGS"], f"-fPIC {native_flag}")
                self.assertEqual(flags["CXXFLAGS"], f"-fPIC {native_flag}")
                self.assertEqual(flags["CARGO_BUILD_TARGET"], {"amd64": "x86_64-unknown-linux-gnu", "arm64": "aarch64-unknown-linux-gnu"}[arch])
                self.assertEqual((Path(directory) / "target-cpu").read_text().strip(), expected_cpu)
                self.assertEqual((Path(directory) / "target-arch").read_text().strip(), arch)

    def test_invalid_build_cpu_fails_before_running_command(self):
        for arch, cpu in [("arm64", "x86-64-v3"), ("amd64", "neoverse-v2"), ("amd64", "neoverse-n1"), ("amd64", "ampere1a"), ("amd64", "native"), ("arm64", "native"), ("arm", "generic")]:
            result = subprocess.run(["sh", str(DEPLOY / "build-target.sh"), arch, cpu, "echo", "command ran"], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("command ran", result.stdout)

    def test_build_command_exit_status_is_preserved(self):
        result = subprocess.run(["sh", str(DEPLOY / "build-target.sh"), "arm64", "", "sh", "-c", "exit 37"], check=False)
        self.assertEqual(result.returncode, 37)

    def deployment_probe(self, image_arch, target_cpu, host_arch):
        with tempfile.TemporaryDirectory() as directory:
            task_dir = Path(directory)
            shutil.copy(DEPLOY / "deploy.sh", task_dir)
            shutil.copy(DEPLOY / "check-cpu.sh", task_dir)
            (task_dir / "pgbouncer").mkdir()
            (task_dir / "pgbouncer/Dockerfile").touch()
            (task_dir / "compose.production.yaml").touch()
            # A local PII key (dummy) satisfies deploy.sh's key check, so the probe reaches the CPU gate.
            previous = "AUDENIQ_IMAGE=ghcr.io/test/audeniq@sha256:previous\nPAYOUT_ACCOUNT_KEY=" + "0" * 64 + "\n"
            (task_dir / "production.env").write_text(previous)
            (task_dir / "production.env").chmod(0o600)
            (task_dir / "uname").write_text(f"#!/bin/sh\necho {host_arch}\n")
            (task_dir / "uname").chmod(0o755)
            (task_dir / "docker").write_text('''#!/usr/bin/env python3
import os,sys,json
with open(os.environ['TASK_DOCKER_LOG'], 'a') as log: log.write(json.dumps(sys.argv[1:])+'\\n')
args=sys.argv[1:]
if args[:2]==['volume','inspect']: sys.exit(1)
if args[:2]==['image','inspect']: print(os.environ['TASK_IMAGE_ARCH'])
if args[:1]==['run'] and '/usr/local/share/audeniq/target-cpu' in args[-1]: print(os.environ['TASK_TARGET_CPU'])
if 'upload-sandbox-check' in args: sys.exit(23)
''')
            (task_dir / "docker").chmod(0o755)
            log = task_dir / "docker-log"
            result = subprocess.run(
                ["bash", str(task_dir / "deploy.sh"), "ghcr.io/test/audeniq@sha256:" + "a" * 64],
                env=dict(os.environ, PATH=str(task_dir) + os.pathsep + os.environ["PATH"], TASK_DOCKER_LOG=str(log), TASK_IMAGE_ARCH=image_arch, TASK_TARGET_CPU=target_cpu),
                capture_output=True, text=True, check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((task_dir / "production.env").read_text(), previous)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            self.assertFalse(any(call[:1] == ["compose"] for call in calls))
            return calls, result

    def test_deploy_rejects_wrong_cpu_before_sandbox_and_migrations(self):
        for arch, cpu, host in [("arm64", "generic", "x86_64"), ("arm64", "x86-64-v3", "aarch64"), ("amd64", "neoverse-v2", "x86_64")]:
            calls, _ = self.deployment_probe(arch, cpu, host)
            self.assertFalse(any("upload-sandbox-check" in call for call in calls))

    def test_portable_deploy_reaches_sandbox_on_both_architectures(self):
        for arch, cpu, host in [("amd64", "x86-64", "x86_64"), ("arm64", "generic", "aarch64")]:
            calls, result = self.deployment_probe(arch, cpu, host)
            self.assertTrue(any("upload-sandbox-check" in call for call in calls), result.stderr)


if __name__ == "__main__":
    unittest.main()
