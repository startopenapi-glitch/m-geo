#!/usr/bin/env python3
"""Static PE / extracted NSIS checks. Never claims to execute or install Windows code."""
import argparse
import hashlib
import json
import re
import struct
from pathlib import Path


def sha(data):
    return hashlib.sha256(data).hexdigest()


def verify_bundle_marker(original, bundled):
    """Permit Tauri's single three-byte bundle-kind patch, and no other change."""
    if bundled == original:
        return "Executable identical without normalization"
    source_marker = b"__TAURI_BUNDLE_TYPE_VAR_UNK"
    bundled_marker = b"__TAURI_BUNDLE_TYPE_VAR_NSS"
    assert len(bundled) == len(original), "Installer executable size differs from build"
    assert original.count(source_marker) == 1, "Expected exactly one source bundle marker"
    assert bundled.count(bundled_marker) == 1, "Expected exactly one NSIS bundle marker"
    offset = original.index(source_marker)
    assert bundled[:offset] == original[:offset], "Executable bytes before bundle marker differ"
    assert bundled[offset:offset + len(bundled_marker)] == bundled_marker, "Unexpected bundle marker"
    assert bundled[offset + len(bundled_marker):] == original[offset + len(source_marker):], "Executable bytes after bundle marker differ"
    return "Only the standard UNK to NSS marker differs"


def inspect_pe(path):
    data = path.read_bytes()
    assert data[:2] == b"MZ", f"Not a PE file: {path}"
    offset = struct.unpack_from("<I", data, 0x3C)[0]
    assert data[offset:offset + 4] == b"PE\0\0", "Missing PE signature"
    machine, sections = struct.unpack_from("<HH", data, offset + 4)
    optional_size = struct.unpack_from("<H", data, offset + 20)[0]
    optional = offset + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directories = optional + (112 if magic == 0x20B else 96)
    section_offset = optional + optional_size
    table = []
    for index in range(sections):
        pos = section_offset + index * 40
        virtual_size, rva, raw_size, raw_pointer = struct.unpack_from("<IIII", data, pos + 8)
        table.append((rva, max(virtual_size, raw_size), raw_pointer))

    def raw(rva):
        for base, size, pointer in table:
            if base <= rva < base + size:
                return pointer + rva - base
        raise AssertionError(f"Unmapped resource RVA {rva:x}")

    resource_rva, resource_size = struct.unpack_from("<II", data, directories + 16)
    assert resource_rva and resource_size, "PE has no resources"
    root = raw(resource_rva)
    resources = []

    def walk(relative, parents=()):
        assert len(parents) < 4, "Unexpected resource tree depth"
        pos = root + relative
        named, ids = struct.unpack_from("<HH", data, pos + 12)
        for index in range(named + ids):
            key, value = struct.unpack_from("<II", data, pos + 16 + index * 8)
            if key & 0x80000000:
                name_pos = root + (key & 0x7FFFFFFF)
                length = struct.unpack_from("<H", data, name_pos)[0]
                key = data[name_pos + 2:name_pos + 2 + length * 2].decode("utf-16le")
            keys = parents + (key,)
            if value & 0x80000000:
                walk(value & 0x7FFFFFFF, keys)
            else:
                rva, size, codepage, _ = struct.unpack_from("<IIII", data, root + value)
                payload = data[raw(rva):raw(rva) + size]
                assert len(payload) == size
                resources.append((keys, payload))

    walk(0)
    cert_offset, cert_size = struct.unpack_from("<II", data, directories + 32)
    imports = []
    import_rva, import_size = struct.unpack_from("<II", data, directories + 8)
    if import_rva and import_size:
        pos = raw(import_rva)
        while any(data[pos:pos + 20]):
            name_rva = struct.unpack_from("<I", data, pos + 12)[0]
            name_pos = raw(name_rva)
            imports.append(data[name_pos:data.index(b"\0", name_pos)].decode("ascii"))
            pos += 20
    delay_imports = []
    delay_rva, delay_size = struct.unpack_from("<II", data, directories + 13 * 8)
    if delay_rva and delay_size:
        pos = raw(delay_rva)
        while any(data[pos:pos + 32]):
            attributes, name_rva = struct.unpack_from("<II", data, pos)
            assert attributes & 1, "Unsupported legacy delay import address format"
            name_pos = raw(name_rva)
            delay_imports.append(data[name_pos:data.index(b"\0", name_pos)].decode("ascii"))
            pos += 32
    summary = {
        "file": path.name, "bytes": len(data), "sha256": sha(data),
        "machine": hex(machine), "pe_format": hex(magic),
        "subsystem": struct.unpack_from("<H", data, optional + 68)[0],
        "authenticode_certificate_present": bool(cert_offset and cert_size),
        "imports": imports,
        "delay_imports": delay_imports,
        "external_vc_runtime_imports": [name for name in imports + delay_imports if re.match(r"^(vcruntime\d|msvcp\d|msvcr\d|concrt\d)", name.lower())],
        "resource_counts": {str(kind): sum(keys[0] == kind for keys, _ in resources)
                            for kind in sorted({keys[0] for keys, _ in resources}, key=str)},
    }
    return summary, resources


def main():
    if not __debug__:
        raise RuntimeError("Do not run package verification with Python assertions disabled")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--installer", type=Path, required=True)
    parser.add_argument("--icon", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--extracted", type=Path)
    parser.add_argument("--resources", type=Path)
    args = parser.parse_args()
    app, app_resources = inspect_pe(args.app)
    installer, installer_resources = inspect_pe(args.installer)
    assert app["machine"] == "0x8664" and app["pe_format"] == "0x20b", "App must be x64 PE32+"
    assert app["subsystem"] == 2, "App must use Windows GUI subsystem"
    assert not app["external_vc_runtime_imports"], "Unexpected separately installed Visual C++ runtime dependency"
    assert installer["machine"] in ("0x14c", "0x8664"), "Unexpected installer architecture"
    ico = args.icon.read_bytes()
    reserved, kind, count = struct.unpack_from("<HHH", ico)
    assert reserved == 0 and kind == 1 and count > 0
    icon_hashes = []
    for index in range(count):
        size, offset = struct.unpack_from("<II", ico, 6 + 16 * index + 8)
        icon_hashes.append(sha(ico[offset:offset + size]))
    for name, summary, entries in [("app", app, app_resources), ("installer", installer, installer_resources)]:
        embedded = {sha(payload) for keys, payload in entries if keys[0] == 3}
        assert set(icon_hashes).issubset(embedded), f"{name}: ICO frames missing from PE resources"
        assert any(keys[0] == 14 for keys, _ in entries), f"{name}: missing icon group"
        version_payload = b"".join(payload for keys, payload in entries if keys[0] == 16)
        assert args.version.encode("utf-16le") in version_payload, f"{name}: version resource mismatch"
        summary["original_logo_frames_verified"] = len(icon_hashes)
        summary["version_verified"] = args.version
    result = {"scope": "static package inspection only; no Windows runtime validation", "app": app, "installer": installer}
    if args.extracted:
        paths = sorted(p for p in args.extracted.rglob("*") if p.is_file())
        payload_apps = [p for p in paths if p.name == args.app.name]
        assert len(payload_apps) == 1, "Expected one main executable in installer"
        original = args.app.read_bytes()
        bundled = payload_apps[0].read_bytes()
        # Tauri patches this 3-byte marker to NSS while packaging and restores UNK
        # afterwards. Require byte-for-byte equality except this exact known patch.
        result["tauri_bundle_type_patch"] = verify_bundle_marker(original, bundled)
        result["installed_executable_sha256"] = sha(bundled)
        assert args.resources, "--resources required with --extracted"
        verified = []
        for source in sorted(args.resources.iterdir()):
            if source.is_file():
                matches = [p for p in paths if p.name == source.name and p.parent.name == "resources"]
                assert len(matches) == 1 and sha(matches[0].read_bytes()) == sha(source.read_bytes()), f"Resource mismatch: {source.name}"
                verified.append(source.name)
        forbidden = [str(p.relative_to(args.extracted)) for p in paths
                     if p.suffix.lower() in (".sqlite", ".sqlite3", ".db", ".mgeobak") or p.name == ".env"]
        assert not forbidden, f"Unexpected private data in installer: {forbidden}"
        result["extracted_files"] = [p.relative_to(args.extracted).as_posix() for p in paths]
        result["bundled_resources_verified"] = verified
        result["database_or_backup_payloads"] = 0
        uninstaller, uninstall_resources = inspect_pe(args.extracted / "uninstall.exe")
        uninstall_icons = {sha(payload) for keys, payload in uninstall_resources if keys[0] == 3}
        assert set(icon_hashes).issubset(uninstall_icons), "Uninstaller is missing original logo frames"
        uninstaller["original_logo_frames_verified"] = len(icon_hashes)
        result["uninstaller"] = uninstaller
    print(json.dumps(result, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
