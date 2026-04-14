const fs = require('fs');

const ADMIN = "GDHO63RZEUNDRVF6WA7HD4D7PLNLUMSK5H74ONW3MEF3VKF4BZJ6GDML";

const rawCurrencies = ["NGN", "ZAR", "KES", "EGP", "GHS", "RWF", "XOF", "MAD", "TZS", "UGX"];

// Try {"SorobanString": "NGN"}
const currencies = rawCurrencies.map(c => { return {"SorobanString": c}; });

const rawWeights = { "NGN": 18, "ZAR": 15, "KES": 12, "EGP": 11, "GHS": 9, "RWF": 8, "XOF": 8, "MAD": 7, "TZS": 6, "UGX": 6 };

const weights = Object.entries(rawWeights).map(([k, v]) => {
  return {
    key: {"SorobanString": k},
    val: v
  };
});

fs.writeFileSync('validators.json', JSON.stringify([ADMIN]));
fs.writeFileSync('currencies.json', JSON.stringify(currencies));
fs.writeFileSync('weights.json', JSON.stringify(weights));

console.log("JSON files created.");
