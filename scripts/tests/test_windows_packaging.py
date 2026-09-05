import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("vt_report", ROOT / "scripts/virustotal_scan.py")
import sys
vt = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = vt
spec.loader.exec_module(vt)

class WindowsReleaseReportTest(unittest.TestCase):
    def test_optional_component_is_scanned_and_vendor_evidence_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Stashi-Wallet-windows-component-i2pd.exe").write_bytes(b"fixture")
            # The same selector used in CI must include independently downloadable helpers.
            files = vt.collect_files(root)
            self.assertEqual([p.name for p in files], ["Stashi-Wallet-windows-component-i2pd.exe"])
            vt.write_reports(root / "report", [{
                "name": files[0].name, "analysis_status": "completed",
                "vendor_detections": {"ESET": {"category": "malicious", "result": "Riskware.I2PD"}},
            }])
            self.assertIn("ESET: Riskware.I2PD", (root / "report/virustotal-results.md").read_text())

if __name__ == '__main__':
    unittest.main()
