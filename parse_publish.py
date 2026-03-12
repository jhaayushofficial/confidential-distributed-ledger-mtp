import json, sys

with open("/tmp/sui_publish.json") as f:
    data = json.load(f)

for obj in data.get("objectChanges", []):
    if obj.get("type") == "published":
        print("PACKAGE_ID=" + obj["packageId"])
    if obj.get("type") == "created" and "LoanLedger" in obj.get("objectType", ""):
        print("LEDGER_OBJECT_ID=" + obj["objectId"])
        print("LEDGER_VERSION=" + str(obj.get("version", "")))
