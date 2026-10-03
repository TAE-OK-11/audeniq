import importlib.util
import pathlib
import tempfile
import unittest

from PIL import Image, PngImagePlugin

helper_path = pathlib.Path(__file__).resolve().parents[1] / "sanitize-upload.py"
spec = importlib.util.spec_from_file_location("sanitize_upload", helper_path)
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class SanitizerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def test_png_metadata_and_appended_payload_are_removed(self):
        source, target = self.root / "source.png", self.root / "safe.png"
        info = PngImagePlugin.PngInfo()
        info.add_text("Script", "MALICIOUS_MARKER")
        Image.new("RGB", (32, 32), "red").save(source, pnginfo=info)
        with source.open("ab") as file:
            file.write(b"<script>MALICIOUS_MARKER</script>")
        helper.sanitize(source, target, "image/png")
        self.assertNotIn(b"MALICIOUS_MARKER", target.read_bytes())
        with Image.open(target) as result:
            self.assertEqual(result.size, (32, 32))
            self.assertEqual(result.getpixel((0, 0)), (255, 0, 0))
            self.assertEqual(result.info, {})

    def test_animated_and_oversized_images_are_rejected(self):
        source, target = self.root / "animated.png", self.root / "safe.png"
        Image.new("RGB", (4, 4), "red").save(
            source, save_all=True, append_images=[Image.new("RGB", (4, 4), "blue")], duration=10)
        with self.assertRaises(ValueError):
            helper.sanitize(source, target, "image/png")
        Image.new("RGB", (8001, 1)).save(source)
        with self.assertRaises(ValueError):
            helper.sanitize(source, target, "image/png")

    def test_html_and_archives_are_never_accepted(self):
        source = self.root / "source"
        source.write_bytes(b"<script>alert(1)</script>")
        for mime in ["text/html", "image/svg+xml", "application/zip", "application/msword"]:
            with self.assertRaises(ValueError):
                helper.sanitize(source, self.root / "safe", mime)

    def test_pdf_javascript_and_embedded_files_do_not_reach_derivative(self):
        source, target = self.root / "source.pdf", self.root / "safe.pdf"
        objects = [
            b"<< /Type /Catalog /Pages 2 0 R /OpenAction 5 0 R /Names << /EmbeddedFiles 6 0 R >> >>",
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>",
            b"<< /Length 23 >>\nstream\n0 0 100 100 re 0.5 g f\nendstream",
            b"<< /S /JavaScript /JS (app.alert('MALICIOUS_MARKER')) >>",
            b"<< /Names [(evil.js) 7 0 R] >>",
            b"<< /Type /Filespec /F (evil.js) /EF << /F 8 0 R >> >>",
            b"<< /Type /EmbeddedFile /Length 16 >>\nstream\nMALICIOUS_MARKER\nendstream",
        ]
        data = bytearray(b"%PDF-1.4\n")
        offsets = [0]
        for n, obj in enumerate(objects, 1):
            offsets.append(len(data))
            data += f"{n} 0 obj\n".encode() + obj + b"\nendobj\n"
        start = len(data)
        data += f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode()
        for offset in offsets[1:]:
            data += f"{offset:010} 00000 n \n".encode()
        data += f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n".encode()
        source.write_bytes(data)
        helper.sanitize(source, target, "application/pdf")
        for forbidden in [b"/JavaScript", b"/OpenAction", b"/EmbeddedFile", b"MALICIOUS_MARKER"]:
            self.assertNotIn(forbidden, target.read_bytes())
        self.assertIn(b"/Subtype /Image", target.read_bytes())
        info = helper.subprocess.run(["pdfinfo", str(target)], capture_output=True, check=True)
        self.assertIn(b"Pages:           1", info.stdout)


if __name__ == "__main__":
    unittest.main()
