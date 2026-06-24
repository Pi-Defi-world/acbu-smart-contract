#!/usr/bin/env python3
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def load_json(path: Path):
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def validate(path: Path):
    data = load_json(path)
    if isinstance(data, dict):
        raise ValueError(f"{path.name} must be a JSON array")
    if not isinstance(data, list):
        raise ValueError(f"{path.name} must be a JSON array")
    return data


def main() -> int:
    validators_path = ROOT / "validators.json"
    currencies_path = ROOT / "currencies.json"
    weights_path = ROOT / "weights.json"

    validators = validate(validators_path)
    currencies = validate(currencies_path)
    weights = validate(weights_path)

    if not all(isinstance(v, str) for v in validators):
        raise ValueError("validators.json must contain a list of address strings")
    if not all(isinstance(item, dict) and "SorobanString" in item for item in currencies):
        raise ValueError("currencies.json must contain objects with a SorobanString field")
    if not all(isinstance(item, dict) and "key" in item and "val" in item for item in weights):
        raise ValueError("weights.json must contain objects with key/val fields")

    currency_codes = {item["SorobanString"] for item in currencies}
    weight_keys = {item["key"]["SorobanString"] for item in weights}
    if currency_codes != weight_keys:
        raise ValueError("currency codes in currencies.json and weights.json do not match")

    print("config validation passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # noqa: BLE001
        print(f"validation failed: {exc}", file=sys.stderr)
        raise SystemExit(1)
