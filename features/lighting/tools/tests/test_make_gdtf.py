"""python3 -m unittest tools.tests.test_make_gdtf  (from the repo root)"""
import json
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE.parent))
import make_gdtf  # noqa: E402

BASES = REPO / "data" / "gdtf"
MOVING_HEAD_BASE = BASES / "UKing@ZQ02341_150W_Big_Steel_Gun_LED_Moving_Head_Beam@2024-R001.gdtf"
PAR_BASE = BASES / "Chauvet@SlimPAR_Pro_Q_USB@Version_1.gdtf"
HAZER_BASE = BASES / "Hazebase@Base_Hazer_Pro@rev1.gdtf"


def parse(out):
    with zipfile.ZipFile(out) as z:
        names = z.namelist()
        root = ET.fromstring(z.read("description.xml"))
    return names, root.find("FixtureType")


@unittest.skipUnless(MOVING_HEAD_BASE.exists(), "base gdtf files absent")
class MakeGdtf(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.out = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_moving_head(self):
        out, = make_gdtf.generate(HERE / "spec_moving_head.json", MOVING_HEAD_BASE, self.out)
        names, ft = parse(out)
        self.assertEqual(out.name, "Betopper@Betopper_150W_beam@ignition.gdtf")
        self.assertTrue(any(n.startswith("models/") for n in names), names)
        self.assertEqual(ft.get("Name"), "Betopper 150W beam")
        self.assertEqual(ft.get("FixtureTypeID"), make_gdtf.fixture_type_id("Betopper 150W beam"))
        modes = ft.find("DMXModes")
        self.assertEqual([m.get("Name") for m in modes], ["14ch", "9ch"])
        ch = modes[0].find("DMXChannels")
        pan = ch[0]
        self.assertEqual(pan.get("Offset"), "1,2")
        self.assertEqual(pan.find("LogicalChannel").get("Attribute"), "Pan")
        self.assertEqual(pan.find("LogicalChannel/ChannelFunction").get("DMXFrom"), "0/2")
        self.assertEqual(len(ch), 12, "PanFine/TiltFine fold into their coarse channel")
        strobe = [c for c in ch if c.find("LogicalChannel").get("Attribute") == "Shutter1"][0]
        attrs = [f.get("Attribute") for f in strobe.iter("ChannelFunction")]
        self.assertIn("Shutter1Strobe", attrs)
        hz = [f for f in strobe.iter("ChannelFunction") if f.get("Attribute") == "Shutter1Strobe"][0]
        self.assertEqual((hz.get("PhysicalFrom"), hz.get("PhysicalTo")), ("1.000000", "20.000000"))
        color = [c for c in ch if c.find("LogicalChannel").get("Attribute") == "Color1"][0]
        sets = list(color.iter("ChannelSet"))
        self.assertEqual(sets[1].get("Name"), "Red")
        self.assertEqual(sets[1].get("WheelSlotIndex"), "2")
        self.assertEqual(color.find("LogicalChannel/ChannelFunction").get("Wheel"), "Color Wheel")
        beam = ft.find("Geometries").iter("Beam").__next__()
        self.assertEqual(beam.get("BeamAngle"), "3.000000")
        self.assertEqual(beam.get("PowerConsumption"), "180.000000")
        defined = {a.get("Name") for a in ft.find("AttributeDefinitions/Attributes")}
        used = {f.get("Attribute") for f in ft.iter("ChannelFunction")} | {l.get("Attribute") for l in ft.iter("LogicalChannel")}
        self.assertTrue(used <= defined, used - defined)
        geometries = {g.get("Name") for g in ft.find("Geometries").iter()}
        for c in ft.iter("DMXChannel"):
            self.assertIn(c.get("Geometry"), geometries)

    def test_par_emitters(self):
        out, = make_gdtf.generate(HERE / "spec_par.json", PAR_BASE, self.out)
        _, ft = parse(out)
        emitters = [e.get("Name") for e in ft.find("PhysicalDescriptions/Emitters")]
        self.assertEqual(emitters, ["Red", "Green", "Blue", "White", "Amber", "UV"])
        uv = ft.find("PhysicalDescriptions/Emitters")[5]
        self.assertIsNone(uv.get("Color"))
        red = [c for c in ft.iter("DMXChannel") if c.find("LogicalChannel").get("Attribute") == "ColorAdd_R"][0]
        self.assertEqual(red.find("LogicalChannel/ChannelFunction").get("Emitter"), "Red")
        self.assertEqual(ft.find("PhysicalDescriptions/ColorSpace").get("Mode"), "sRGB")

    def test_moving_head_needs_axes(self):
        spec = json.loads((HERE / "spec_moving_head.json").read_text())
        p = self.out / "s.json"
        p.write_text(json.dumps(spec))
        with self.assertRaises(make_gdtf.SpecError):
            make_gdtf.generate(p, HAZER_BASE, self.out)

    def test_unknown_attribute_becomes_user_defined(self):
        spec = json.loads((HERE / "spec_par.json").read_text())
        spec["modes"]["3ch"][0]["attribute"] = "Bogus Thing"
        p = self.out / "s.json"
        p.write_text(json.dumps(spec))
        gen = make_gdtf.Generator(spec, make_gdtf.Base(PAR_BASE))
        ft = gen.build().find("FixtureType")
        self.assertTrue(any("outside the vocabulary" in w for w in gen.warnings), gen.warnings)
        self.assertIn("BogusThing", {a.get("Name") for a in ft.find("AttributeDefinitions/Attributes")})

    def test_compressed_bar_mode_expands(self):
        spec = json.loads((HERE / "spec_par.json").read_text())
        spec["modes"] = {"24ch": [{"channel": "1-24", "attribute": "Red/Green/Blue per section"}],
                         "30ch": [{"channel": "1-24", "attribute": "as 24ch"},
                                  {"channel": "25-30", "attribute": "Dimmer (per section)"}],
                         "30ch_guess": True}
        p = self.out / "s.json"
        p.write_text(json.dumps(spec))
        out, = make_gdtf.generate(p, REPO / "data" / "gdtf" /
                                  "American_DJ@Ultra_Bar_12@Close_-_Needs_Work_on_Strobes_and_Programs.gdtf", self.out)
        _, ft = parse(out)
        modes = {m.get("Name"): m for m in ft.find("DMXModes")}
        self.assertEqual(set(modes), {"24ch", "30ch"})
        attrs = [c.find("LogicalChannel").get("Attribute") for c in modes["30ch"].iter("DMXChannel")]
        self.assertEqual(attrs[:3], ["ColorAdd_R", "ColorAdd_G", "ColorAdd_B"])
        self.assertEqual(len(attrs), 30)
        geoms = [c.get("Geometry") for c in modes["24ch"].iter("DMXChannel")]
        self.assertEqual(len(set(geoms)), 8, "one beam per RGB section")


if __name__ == "__main__":
    unittest.main()
