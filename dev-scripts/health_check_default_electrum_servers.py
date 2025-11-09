#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""
Check electrum server availability.
Usage: uv run check_electrum_servers.py
"""

import json
import re
import socket
import ssl
import sys
from pathlib import Path
from urllib.parse import urlparse
from typing import List, Tuple
from concurrent.futures import ThreadPoolExecutor, as_completed


def extract_servers_from_rust(file_path: Path) -> List[str]:
    """Extract electrum server URLs from Rust defaults.rs file."""
    servers = []
    content = file_path.read_text()

    # Find all Url::parse() calls
    pattern = r'Url::parse\("([^"]+)"\)'
    matches = re.findall(pattern, content)
    servers.extend(matches)

    return servers


def extract_servers_from_typescript(file_path: Path) -> List[str]:
    """Extract electrum server URLs from TypeScript defaults.ts file."""
    servers = []
    content = file_path.read_text()

    # Find all server strings in the arrays
    pattern = r'"((?:tcp|ssl)://[^"]+)"'
    matches = re.findall(pattern, content)
    servers.extend(matches)

    return servers


def check_tcp_server(host: str, port: int, timeout: int = 2) -> Tuple[bool, str]:
    """Check TCP electrum server."""
    request = json.dumps({"id": 1, "method": "blockchain.headers.subscribe", "params": []}) + "\n"

    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(timeout)
            sock.connect((host, port))
            sock.sendall(request.encode())

            response = b""
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                response += chunk
                if b"\n" in response:
                    break

            response_str = response.decode().strip()
            if response_str:
                data = json.loads(response_str)
                if 'result' in data or 'error' in data:
                    return True, "OK"

        return False, "No valid response"
    except socket.timeout:
        return False, "Timeout"
    except ConnectionRefusedError:
        return False, "Connection refused"
    except Exception as e:
        return False, str(e)


def check_ssl_server(host: str, port: int, timeout: int = 2) -> Tuple[bool, str]:
    """Check SSL/TLS electrum server."""
    request = json.dumps({"id": 1, "method": "blockchain.headers.subscribe", "params": []}) + "\n"

    try:
        context = ssl.create_default_context()
        # Don't verify certificates for testing purposes
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(timeout)
            with context.wrap_socket(sock, server_hostname=host) as ssock:
                ssock.connect((host, port))
                ssock.sendall(request.encode())

                response = b""
                while True:
                    chunk = ssock.recv(4096)
                    if not chunk:
                        break
                    response += chunk
                    if b"\n" in response:
                        break

                response_str = response.decode().strip()
                if response_str:
                    data = json.loads(response_str)
                    if 'result' in data or 'error' in data:
                        return True, "OK"

        return False, "No valid response"
    except socket.timeout:
        return False, "Timeout"
    except ConnectionRefusedError:
        return False, "Connection refused"
    except ssl.SSLError as e:
        return False, f"SSL error: {str(e)}"
    except Exception as e:
        return False, str(e)


def check_server(url: str) -> Tuple[str, bool, str]:
    """Check a single electrum server."""
    parsed = urlparse(url)
    protocol = parsed.scheme
    host = parsed.hostname
    port = parsed.port

    if not host or not port:
        return url, False, "Invalid URL format"

    if protocol == "tcp":
        success, message = check_tcp_server(host, port)
    elif protocol == "ssl":
        success, message = check_ssl_server(host, port)
    else:
        return url, False, f"Unknown protocol: {protocol}"

    return url, success, message


def main():
    # Find the files
    base_dir = Path(__file__).parent
    rust_file = base_dir / "swap-env" / "src" / "defaults.rs"
    ts_file = base_dir / "src-gui" / "src" / "store" / "features" / "defaults.ts"

    if not rust_file.exists():
        print(f"❌ Rust defaults file not found: {rust_file}")
        sys.exit(1)

    if not ts_file.exists():
        print(f"❌ TypeScript defaults file not found: {ts_file}")
        sys.exit(1)

    # Extract servers
    print("📋 Extracting server URLs...")
    rust_servers = extract_servers_from_rust(rust_file)
    ts_servers = extract_servers_from_typescript(ts_file)

    # Combine and deduplicate
    all_servers = list(set(rust_servers + ts_servers))
    all_servers.sort()

    print(f"\n📊 Found {len(all_servers)} unique servers\n")

    # Check servers in parallel
    print("🔍 Checking servers (this may take a minute)...\n")

    working = []
    broken = []

    with ThreadPoolExecutor(max_workers=10) as executor:
        futures = {executor.submit(check_server, url): url for url in all_servers}

        for future in as_completed(futures):
            url, success, message = future.result()

            status = "✅" if success else "❌"
            print(f"{status} {url:60s} {message}")

            if success:
                working.append(url)
            else:
                broken.append((url, message))

    # Summary
    print("\n" + "="*80)
    print(f"\n📊 Summary:")
    print(f"   ✅ Working: {len(working)}/{len(all_servers)}")
    print(f"   ❌ Broken:  {len(broken)}/{len(all_servers)}")

    if broken:
        print(f"\n⚠️  Broken servers:")
        for url, reason in broken:
            print(f"   • {url} - {reason}")

    # Check if the newly added server is working
    new_server = "tcp://bitcoin.aranguren.org:50001"
    if new_server in all_servers:
        is_working = new_server in working
        status = "✅ WORKING" if is_working else "❌ BROKEN"
        print(f"\n🆕 Newly added server: {new_server} - {status}")

    sys.exit(0 if len(broken) == 0 else 1)


if __name__ == "__main__":
    main()
