"""Converts LICENSE to installer/wix/license.rtf (needed by the MSI license page)."""
import os

root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
text = open(os.path.join(root, "..", "LICENSE"), encoding="utf-8").read()  # repo root
body = text.replace("\\", "\\\\").replace("{", "\\{").replace("}", "\\}").replace("\n", "\\par\n")
rtf = "{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Consolas;}}\\fs16\n" + body + "}"
with open(os.path.join(root, "installer", "wix", "license.rtf"), "w", encoding="ascii", errors="replace") as f:
    f.write(rtf)
print("license.rtf written")
