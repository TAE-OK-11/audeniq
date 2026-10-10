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

    def test_arm_checks_all_cores_and_requires_cpu_identity(self):
        for records in ["", "Features : " + ARM_V2, arm_record() + arm_record("0xd40"), arm_record().replace("CPU implementer : 0x41", "CPU implementer : 0x42")]:
            self.check_cpu("arm64", "neoverse-v2", "aarch64", records, succeeds=False)

    def test_build_flags_apply_to_rust_and_native_dependencies(self):
        for arch, cpu, expected_cpu, native_flag in [
            ("amd64", "", "x86-64", "-march=x86-64"),
            ("amd64", "x86-64-v3", "x86-64-v3", "-march=x86-64-v3"),
            ("arm64", "", "generic", "-march=armv8-a"),
            ("arm64", "neoverse-v2", "neoverse-v2", "-mcpu=neoverse-v2"),
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
        for arch, cpu in [("arm64", "x86-64-v3"), ("amd64", "neoverse-v2"), ("amd64", "native"), ("arm64", "native"), ("arm", "generic")]:
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
            previous = "AUDENIQ_IMAGE=ghcr.io/test/audeniq@sha256:previous\n"
            (task_dir / "production.env").write_text(previous)
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
                ["bash", str(task_dir / "deploy.sh"), "ghcr.io/test/audeniq@sha256:new"],
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
