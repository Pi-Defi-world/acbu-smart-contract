const fs = require('fs');
const { execFileSync } = require('child_process');
const path = require('path');

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

const rootDir = path.resolve(__dirname, '..');
fs.writeFileSync(path.join(rootDir, 'validators.json'), JSON.stringify([ADMIN]));
fs.writeFileSync(path.join(rootDir, 'currencies.json'), JSON.stringify(currencies));
fs.writeFileSync(path.join(rootDir, 'weights.json'), JSON.stringify(weights));

execFileSync(process.execPath, [path.join(rootDir, 'scripts', 'validate_config.py')], {
  cwd: rootDir,
  stdio: 'inherit',
});

// log
console.log('JSON files created and validated.');
