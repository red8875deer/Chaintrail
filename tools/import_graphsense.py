#!/usr/bin/env python3
"""Convert an allowlisted subset of GraphSense TagPacks into ChainTrail label packs.

Usage:
    git clone --depth 1 https://github.com/graphsense/graphsense-tagpacks /tmp/gs
    python3 tools/import_graphsense.py /tmp/gs data/labels

Only packs listed in PACKS are imported. Each one was reviewed for where its data comes
from (docs/DATA_SOURCES.md): governments, the entity itself, or published research.
Packs derived from sites whose terms forbid reuse (e.g. explorer label pages) are listed in
EXCLUDED with the reason. The tool also writes data/labels/SOURCES.md.

Requires PyYAML. Addresses are validated (checksums included); invalid ones are skipped
and reported. Only Bitcoin, Ethereum and Tron addresses are kept.
"""

import glob
import hashlib
import json
import os
import subprocess
import sys

import yaml

GRAPHSENSE_REPO = "https://github.com/graphsense/graphsense-tagpacks"
VIA = "GraphSense TagPacks (MIT licence), " + GRAPHSENSE_REPO

# GraphSense confidence levels -> ours.
CONFIDENCE = {
    "ownership": "high",
    "authority_data": "high",
    "service_data": "high",
    "ledger_immanent": "high",
    "forensic": "medium",
    "web_crawl": "low",
    "untrusted_transaction": "low",
    None: "medium",
}

# GraphSense categories -> ours.
CATEGORY = {
    "exchange": "exchange",
    "mixing_service": "mixer",
    "coinjoin": "mixer",
    "miner": "mining_pool",
    "market": "darknet",
    "defi": "defi",
    "defi_lending": "defi",
    "defi_dex": "dex",
    "defi_bridge": "bridge",
    "bridge": "bridge",
    "gambling": "gambling",
    "wallet_service": "wallet_service",
    "payment_processor": "payment_processor",
    "service": "unknown_service",
    "user": "individual",
}

# GraphSense abuse types -> ours. Abuse wins over category: it says what the address did.
ABUSE = {
    "ransomware": "ransomware",
    "scam": "scam",
    "phishing": "scam",
    "ponzi_scheme": "scam",
    "sextortion": "scam",
    "service_hack": "hack",
    "hack": "hack",
    "terrorism": "terrorist_financing",
    "sanction": "sanctioned",
}

# Reviewed packs. Optional keys: entity (fixed entity name; the GraphSense label becomes
# the note), category / confidence (overrides), label_category (per-label override),
# title.
PACKS = [
    # Exchanges' own published reserve wallet lists.
    dict(file="exchange-wallets-bitfinexcom.yaml", id="gs-bitfinex-reserves", entity="Bitfinex"),
    dict(file="exchange-wallets-okx.yaml", id="gs-okx-reserves", entity="OKX"),
    dict(file="exchange-wallets-huobi.yaml", id="gs-huobi-reserves", entity="Huobi (HTX)"),
    dict(file="exchange-wallets-binance.yaml", id="gs-binance-reserves", entity="Binance"),
    dict(file="exchange-wallets-kucoin.yaml", id="gs-kucoin-reserves", entity="KuCoin"),
    dict(file="exchange-wallets-bybit.yaml", id="gs-bybit-reserves", entity="Bybit"),
    dict(file="exchange-wallets-deribit.yaml", id="gs-deribit-reserves", entity="Deribit"),
    dict(file="exchange-wallets-cryptocom.yaml", id="gs-cryptocom-reserves", entity="Crypto.com"),
    dict(file="service-wallets-checksig.yaml", id="gs-checksig-reserves", entity="Checksig"),
    # Binance addresses identified from the 2019 hack (news report): medium.
    dict(file="binance.yaml", id="gs-binance-2019", entity="Binance", confidence="medium"),
    # Mixers and swap services.
    dict(file="mixing_fc2021.yaml", id="gs-mixers-fc2021"),
    dict(file="wasabi_collector.yaml", id="gs-wasabi-fees",
         entity="Wasabi Wallet CoinJoin coordinator (fee address)", category="mixer"),
    dict(file="blender_io.yaml", id="gs-blender-io", entity="Blender.io"),
    dict(file="sinbad_io.yaml", id="gs-sinbad-io", entity="Sinbad.io"),
    dict(file="tornado_cash.yaml", id="gs-tornado-cash", entity="Tornado Cash"),
    # SideShift is an instant swap service, not a mixer.
    dict(file="interpol-real_services.yaml", id="gs-interpol-services",
         label_category={"sideshift.ai": "swap_service"}),
    # Darknet markets.
    dict(file="hydra.yaml", id="gs-hydra-market", entity="Hydra Market"),
    # Ransomware.
    dict(file="ransomwhere.yaml", id="gs-ransomwhere"),
    dict(file="ransomware.yaml", id="gs-ransomware-2019"),
    # Hacks and thefts.
    dict(file="binance_hack.yaml", id="gs-binance-hack-2019", entity="Binance hack (2019) attacker"),
    dict(file="lazarus.yaml", id="gs-lazarus-ofac", entity="Lazarus Group (DPRK)", category="hack"),
    dict(file="lazarus2.yaml", id="gs-lazarus-fbi", entity="Lazarus Group (DPRK)", category="hack"),
    dict(file="ronin_bridge.yaml", id="gs-ronin-exploit", category="hack"),
    dict(file="africrypt-hack.yaml", id="gs-africrypt"),
    # Scams.
    dict(file="twitter_hack_scam.yaml", id="gs-twitter-hack-2020", entity="Twitter hack scam (2020)"),
    dict(file="electrum_phishing.yaml", id="gs-electrum-phishing"),
    dict(file="ponzi_scheme.yaml", id="gs-ponzi-schemes"),
    dict(file="plustoken.yaml", id="gs-plustoken", entity="PlusToken"),
    # Terrorist financing (DOJ forfeiture complaint).
    dict(file="aft-alqaeda-forfeit_vc.yaml", id="gs-alqaeda-forfeiture",
         entity="Al-Qaeda campaign (DOJ forfeiture)"),
    # Mining pools (published research dataset).
    dict(file="miners.yaml", id="gs-mining-pools"),
    dict(file="miners_additional.yaml", id="gs-mining-pools-2020"),
    # DeFi protocol contracts (published research dataset and official documentation).
    dict(file="defi-protocols-csh.yaml", id="gs-defi-protocols"),
] + [
    dict(file=os.path.join("defi_updated", f), id="gs-" + f[:-5].replace("_", "-").replace("defi-protocols-", "defi-"))
    for f in [
        "defi-protocols_uniswap.yaml",
        "defi-protocols_uniswap_pools.yaml",
        "defi-protocols_aave.yaml",
        "defi-protocols_synthetix.yaml",
        "defi-protocols_instadapp.yaml",
        "defi-protocols_compound.yaml",
        "defi-protocols_sushiswap.yaml",
        "defi-protocols_curve.yaml",
        "defi-protocols_1inch.yaml",
    ]
]

EXCLUDED = {
    "etherscan-label-word-cloud.yaml": "derived from Etherscan labels; Etherscan's terms forbid reuse",
    "etherscan-wordcloud-*.yaml": "derived from Etherscan labels; Etherscan's terms forbid reuse",
    "defi-fraud-masterthesis.yaml": "mostly derived from Etherscan labels",
    "richest_addresses.yaml": "third-party rich list (bitinfocharts), not the entity's own data",
    "chaininfo.yaml": "third-party rich list, not the entity's own data",
    "walletexplorer.yaml": "third-party explorer attributions; terms not yet reviewed",
    "hacks.yaml": "forum and gist sources (web crawl); quality not yet reviewed",
    "exchange-wallets-swissborg.yaml": "taken from a third-party analytics site",
    "BITB_etf.yaml": "news report about a third party",
    "exchange-wallets-bitmex_*.yaml": "336k addresses; too large to embed for now (planned: separate download)",
    "samourai.yaml": "36k CoinJoin participant addresses; CoinJoin transactions are detected by heuristics instead",
    "sextortion_*.yaml": "70k addresses; licence of the original IOC lists not yet reviewed",
    "etherscamdb_tagpack.yaml": "licence of CryptoScamDB data not yet reviewed",
    "usdt_blacklist.yaml": "will be derived directly from Tether contract events (Phase 4)",
    "aidecisions_ofac.yaml": "superseded by our own import of the official OFAC list",
    "ofac.yaml": "superseded by our own import of the official OFAC list",
    "Alt-Right.yaml": "political organisations and individuals; out of scope",
    "lockbit_tattoos.yaml": "web crawl of social media; low quality",
    "forsage.yaml": "web crawl; low quality",
    "coinjoin_bounty.yaml": "untrusted single forum post",
    "eosio-gambling_part*.yaml": "EOS accounts; chain not supported",
    "satoshidice_bch.yaml": "Bitcoin Cash; chain not supported",
    "bitzillions_ltc.yaml": "Litecoin; chain not supported",
    "etherhiding.yaml": "BNB Smart Chain; chain not supported",
    "demo.yaml": "demonstration pack",
    "protonmail.yaml": "single donation address; out of scope",
    "suidlanders.yaml": "organisation donation addresses; out of scope",
}

# ---- address validation ---------------------------------------------------------------

B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def b58check(s):
    n = 0
    for c in s:
        if c not in B58:
            return None
        n = n * 58 + B58.index(c)
    raw = n.to_bytes((n.bit_length() + 7) // 8, "big")
    raw = b"\x00" * (len(s) - len(s.lstrip("1"))) + raw
    if len(raw) < 5:
        return None
    body, check = raw[:-4], raw[-4:]
    if hashlib.sha256(hashlib.sha256(body).digest()).digest()[:4] != check:
        return None
    return body


BECH32 = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"


def bech32_polymod(values):
    gen = [0x3B6A57B2, 0x26508E6D, 0x1EA119FA, 0x3D4233DD, 0x2A1462B3]
    chk = 1
    for v in values:
        top = chk >> 25
        chk = (chk & 0x1FFFFFF) << 5 ^ v
        for i in range(5):
            chk ^= gen[i] if ((top >> i) & 1) else 0
    return chk


def segwit_ok(addr):
    a = addr.lower()
    if addr != a and addr != addr.upper():
        return False
    if not a.startswith("bc1") or len(a) > 90:
        return False
    data = a[3:]
    if any(c not in BECH32 for c in data) or len(data) < 7:
        return False
    vals = [BECH32.index(c) for c in data]
    hrp = [ord(c) >> 5 for c in "bc"] + [0] + [ord(c) & 31 for c in "bc"]
    const = bech32_polymod(hrp + vals)
    version = vals[0]
    if version == 0 and const != 1:
        return False
    if version > 0 and const != 0x2BC830A3:
        return False
    # Convert 5-bit groups to bytes to check the program length.
    acc, bits, prog = 0, 0, []
    for v in vals[1:-6]:
        acc = (acc << 5) | v
        bits += 5
        while bits >= 8:
            bits -= 8
            prog.append((acc >> bits) & 0xFF)
    if bits >= 5 or (acc & ((1 << bits) - 1)):
        return False
    if not 2 <= len(prog) <= 40 or version > 16:
        return False
    return version != 0 or len(prog) in (20, 32)


def classify(addr):
    """Return (chain, normalised address) or None."""
    a = addr.strip()
    if a.startswith(("0x", "0X")):
        body = a[2:]
        if len(body) == 40 and all(c in "0123456789abcdefABCDEF" for c in body):
            # Lowercase: ChainTrail normalises Ethereum addresses (mixed-case checksums are
            # not verified here, so they are dropped rather than trusted).
            return ("ethereum", "0x" + body.lower())
        return None
    if a.startswith("T"):
        b = b58check(a)
        if b is not None and len(b) == 21 and b[0] == 0x41:
            return ("tron", a)
        return None
    if a.lower().startswith("bc1"):
        return ("bitcoin", a.lower()) if segwit_ok(a) else None
    b = b58check(a)
    if b is not None and len(b) == 21 and b[0] in (0x00, 0x05):
        return ("bitcoin", a)
    return None


# ---- conversion -----------------------------------------------------------------------

SUPPORTED_CURRENCIES = {"BTC", "ETH", "TRX", "USDT", "USDC"}


def toml_str(s):
    return json.dumps(str(s), ensure_ascii=False)


def convert(gs_dir, spec):
    path = os.path.join(gs_dir, "packs", spec["file"])
    d = yaml.safe_load(open(path, encoding="utf-8"))
    tags = d.get("tags") or []
    pack_conf = CONFIDENCE.get(d.get("confidence"), "medium")
    groups = {}
    skipped = {"unsupported": 0, "invalid": 0}
    for t in tags:
        cur = (t.get("currency") or d.get("currency") or "").upper()
        if cur not in SUPPORTED_CURRENCIES:
            skipped["unsupported"] += 1
            continue
        parsed = classify(str(t.get("address", "")))
        if parsed is None:
            skipped["invalid"] += 1
            continue
        chain, addr = parsed
        label = t.get("label") or d.get("label") or d.get("title")
        abuse = t.get("abuse") or d.get("abuse")
        cat = (
            spec.get("label_category", {}).get(label)
            or spec.get("category")
            or ABUSE.get(abuse)
            or CATEGORY.get(t.get("category") or d.get("category"))
            or "unknown_service"
        )
        tag_conf = t.get("confidence")
        conf = spec.get("confidence") or (
            CONFIDENCE.get(tag_conf, pack_conf) if tag_conf else pack_conf
        )
        entity = spec.get("entity") or label
        note = label if spec.get("entity") and label and label != entity else None
        source = t.get("source") or d.get("source")
        key = (entity, cat, conf, source, note)
        groups.setdefault(key, {"bitcoin": [], "ethereum": [], "tron": []})
        lst = groups[key][chain]
        if addr not in lst:
            lst.append(addr)
    # Pack-level defaults: the most common category/confidence.
    cats = {}
    confs = {}
    for (e, c, cf, s, n), addrs in groups.items():
        k = sum(len(v) for v in addrs.values())
        cats[c] = cats.get(c, 0) + k
        confs[cf] = confs.get(cf, 0) + k
    pack_cat = max(cats, key=cats.get) if cats else "unknown_service"
    pack_cf = max(confs, key=confs.get) if confs else pack_conf
    source = d.get("source") or (
        GRAPHSENSE_REPO + "/blob/master/packs/" + spec["file"].replace(os.sep, "/")
    )
    count = sum(len(v) for g in groups.values() for v in g.values())
    lines = [
        "# Generated by tools/import_graphsense.py from GraphSense TagPacks; do not edit.",
        "[pack]",
        f"id = {toml_str(spec['id'])}",
        f"title = {toml_str(spec.get('title') or d.get('title') or spec['id'])}",
        f"source = {toml_str(source)}",
        f"publisher = {toml_str(d.get('creator') or 'GraphSense')}",
        f"licence = {toml_str('MIT (GraphSense TagPacks); facts attributed to the original source')}",
    ]
    if d.get("lastmod"):
        lines.append(f"as_of = {toml_str(d['lastmod'])}")
    lines.append(f"via = {toml_str(VIA)}")
    if d.get("description"):
        lines.append(f"description = {toml_str(' '.join(str(d['description']).split()))}")
    lines += [f"category = {toml_str(pack_cat)}", f"confidence = {toml_str(pack_cf)}"]
    for (entity, cat, conf, src, note), addrs in sorted(
        groups.items(), key=lambda kv: (str(kv[0][0]).lower(), str(kv[0][3]))
    ):
        lines += ["", "[[group]]", f"entity = {toml_str(entity)}"]
        if cat != pack_cat:
            lines.append(f"category = {toml_str(cat)}")
        if conf != pack_cf:
            lines.append(f"confidence = {toml_str(conf)}")
        if src and src != source:
            lines.append(f"source = {toml_str(src)}")
        if note:
            lines.append(f"note = {toml_str(note)}")
        for chain in ("bitcoin", "ethereum", "tron"):
            if addrs[chain]:
                lines.append(f"{chain} = [")
                lines += [f"    {toml_str(a)}," for a in addrs[chain]]
                lines.append("]")
    return "\n".join(lines) + "\n", count, skipped, d


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    gs_dir, out_dir = sys.argv[1], sys.argv[2]
    try:
        commit = subprocess.run(
            ["git", "-C", gs_dir, "log", "-1", "--format=%h %cs"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
    except Exception:
        commit = "unknown"
    rows = []
    for spec in PACKS:
        text, count, skipped, d = convert(gs_dir, spec)
        if count == 0:
            print(f"skip {spec['file']}: no supported addresses")
            continue
        with open(os.path.join(out_dir, spec["id"] + ".toml"), "w", encoding="utf-8") as f:
            f.write(text)
        rows.append((spec, count, skipped, d))
        print(f"{spec['id']:<32} {count:>6}  (skipped {skipped})")

    listed = {s["file"] for s in PACKS}
    all_files = sorted(
        os.path.relpath(p, os.path.join(gs_dir, "packs"))
        for p in glob.glob(os.path.join(gs_dir, "packs", "**", "*.yaml"), recursive=True)
    )
    import fnmatch

    unreviewed = [
        f for f in all_files
        if f not in listed and not any(fnmatch.fnmatch(f, pat) for pat in EXCLUDED)
    ]
    md = [
        "# Label sources",
        "",
        "Generated by `tools/import_graphsense.py`; edit the tool, not this file.",
        "",
        "## Imported from GraphSense TagPacks",
        "",
        f"Repository: {GRAPHSENSE_REPO} (MIT), commit {commit}.",
        "Each label keeps a link to its original source.",
        "",
        "| Pack | Labels | Original source | GraphSense confidence |",
        "|---|---:|---|---|",
    ]
    for spec, count, skipped, d in rows:
        md.append(
            f"| `{spec['id']}` | {count} | {d.get('source') or 'per label'} | {d.get('confidence') or 'per label'} |"
        )
    md += ["", "## Not imported, and why", "", "| Pack | Reason |", "|---|---|"]
    for pat, why in sorted(EXCLUDED.items()):
        md.append(f"| `{pat}` | {why} |")
    for f in unreviewed:
        md.append(f"| `{f}` | not yet reviewed |")
    md += [
        "",
        "## Other packs",
        "",
        "- `ofac-sdn`: generated with `chaintrail labels update-ofac` from the official OFAC",
        "  SDN list (public domain). Refresh with the same command.",
        "",
    ]
    with open(os.path.join(out_dir, "SOURCES.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(md))


if __name__ == "__main__":
    main()
