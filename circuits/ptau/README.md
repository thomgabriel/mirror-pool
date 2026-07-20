# Powers of tau (dev/CI only)

`scripts/setup.sh` needs a Groth16 powers-of-tau file at `ptau/pot14_final.ptau`
(2^14 = 16384 constraints — the membership circuit has 5313, so pot14 is ample).

This is the **public Hermez ceremony** phase-1 output, truncated/prepared for
2^14 constraints. It is a well-known, widely reused dev/CI artifact — **not**
a circuit-specific or production ceremony. Do not treat it as a substitute for
a real multi-party trusted setup before any mainnet deployment.

The `.ptau` binary is **not committed** (`*.ptau` is gitignored — it's ~18 MB).
Fetch it and verify its checksum before running `setup.sh`:

```bash
cd circuits/ptau
curl -sL -o pot14_final.ptau \
  https://storage.googleapis.com/zkevm/ptau/powersOfTau28_hez_final_14.ptau
echo "489be9e5ac65d524f7b1685baac8a183c6e77924fdb73d2b8105e335f277895d  pot14_final.ptau" | shasum -a 256 -c -
```

| field  | value |
|---|---|
| file | `powersOfTau28_hez_final_14.ptau` (Hermez, bn128, 2^14) |
| source URL | `https://storage.googleapis.com/zkevm/ptau/powersOfTau28_hez_final_14.ptau` |
| size | 18,957,464 bytes |
| sha256 | `489be9e5ac65d524f7b1685baac8a183c6e77924fdb73d2b8105e335f277895d` |

Always re-verify the sha256 after fetching, regardless of source — do not
trust size alone.

CI should cache `pot14_final.ptau` keyed on the sha256 above rather than
re-fetching every run, and should still re-verify the checksum on a cache
miss.
