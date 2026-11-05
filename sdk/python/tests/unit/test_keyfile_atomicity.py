"""Wallet backups survive incomplete writes, failed publication, and concurrent writers."""

import errno
import os
import stat
from concurrent.futures import ThreadPoolExecutor
from threading import Barrier, Lock

import pytest

from bittensor.keyfiles import Keyfile, Keypair
from bittensor.sp_core import serialized_keypair_to_keyfile_data, verify_mldsa_authorization


def test_short_writes_publish_only_the_complete_keyfile(tmp_path, monkeypatch):
    path = tmp_path / "coldkey"
    path.write_bytes(b"previous recoverable wallet")
    payload = b"new complete recoverable wallet" * 40
    write = os.write

    def short_write(fd, data):
        return write(fd, data[:7])

    monkeypatch.setattr(os, "write", short_write)
    Keyfile(path)._write_data(payload, overwrite=True)
    assert path.read_bytes() == payload
    assert stat.S_IMODE(path.stat().st_mode) == 0o600
    assert list(tmp_path.iterdir()) == [path]


@pytest.mark.parametrize("failure", ["zero", "write", "sync", "replace"])
def test_failure_preserves_existing_wallet_and_removes_temporary_file(
    tmp_path, monkeypatch, failure
):
    path = tmp_path / "coldkey"
    previous = b"previous recoverable wallet"
    path.write_bytes(previous)
    write = os.write
    writes = 0

    def fail(*args):
        raise OSError(errno.ENOSPC, "injected storage failure")

    def partial_then_failure(fd, data):
        nonlocal writes
        writes += 1
        if writes == 1:
            return write(fd, data[:3])
        if failure == "zero":
            return 0
        fail()

    if failure in {"zero", "write"}:
        monkeypatch.setattr(os, "write", partial_then_failure)
    else:
        monkeypatch.setattr(os, "fsync" if failure == "sync" else "replace", fail)
    with pytest.raises(OSError):
        Keyfile(path)._write_data(b"new complete recoverable wallet", overwrite=True)
    assert path.read_bytes() == previous
    assert list(tmp_path.iterdir()) == [path]


@pytest.mark.parametrize("overwrite", [False, True])
def test_concurrent_writers_publish_complete_independent_files(tmp_path, monkeypatch, overwrite):
    path = tmp_path / "coldkey"
    payloads = [b"first wallet" * 100, b"second wallet" * 100]
    write = os.write
    barrier = Barrier(2)
    lock = Lock()
    inodes = set()

    def simultaneous_write(fd, data):
        with lock:
            inodes.add(os.fstat(fd).st_ino)
        barrier.wait(timeout=10)
        return write(fd, data)

    def save(payload):
        try:
            Keyfile(path)._write_data(payload, overwrite=overwrite)
            return True
        except FileExistsError:
            return False

    monkeypatch.setattr(os, "write", simultaneous_write)
    with ThreadPoolExecutor(max_workers=2) as pool:
        outcomes = list(pool.map(save, payloads))
    assert len(inodes) == 2
    assert sum(outcomes) == (2 if overwrite else 1)
    assert path.read_bytes() in payloads
    assert list(tmp_path.iterdir()) == [path]


def test_existing_shared_temp_symlink_is_never_followed(tmp_path):
    victim = tmp_path / "unrelated-backup"
    victim.write_bytes(b"keep this backup")
    path = tmp_path / "coldkey"
    old_temporary = path.with_suffix(".tmp")
    old_temporary.symlink_to(victim)
    Keyfile(path)._write_data(b"complete new wallet", overwrite=True)
    assert victim.read_bytes() == b"keep this backup"
    assert old_temporary.is_symlink()
    assert path.read_bytes() == b"complete new wallet"


@pytest.mark.skipif(os.name != "posix", reason="directory fsync is a POSIX durability step")
def test_publication_syncs_complete_file_before_directory(tmp_path, monkeypatch):
    path = tmp_path / "coldkey"
    path.write_bytes(b"previous wallet")
    payload = b"complete new wallet"
    sync = os.fsync
    synced = []

    def record_sync(fd):
        directory = stat.S_ISDIR(os.fstat(fd).st_mode)
        if directory:
            assert path.read_bytes() == payload
        else:
            assert path.read_bytes() == b"previous wallet"
        synced.append("directory" if directory else "file")
        sync(fd)

    monkeypatch.setattr(os, "fsync", record_sync)
    Keyfile(path)._write_data(payload, overwrite=True)
    assert synced == ["file", "directory"]


@pytest.mark.skipif(os.name != "posix", reason="directory fsync is a POSIX durability step")
def test_directory_sync_failure_reports_error_with_complete_published_wallet(tmp_path, monkeypatch):
    path = tmp_path / "coldkey"
    path.write_bytes(b"previous wallet")
    keypair = Keypair.create_from_seed(bytes([19]) * 32, 8)
    payload = bytes(serialized_keypair_to_keyfile_data(keypair))
    sync = os.fsync
    synced = []

    def fail_directory_sync(fd):
        if stat.S_ISDIR(os.fstat(fd).st_mode):
            synced.append("directory")
            raise OSError(errno.EIO, "injected directory sync failure")
        synced.append("file")
        sync(fd)

    monkeypatch.setattr(os, "fsync", fail_directory_sync)
    keyfile = Keyfile(path)
    with pytest.raises(OSError, match="injected directory sync failure"):
        keyfile._write_data(payload, overwrite=True)
    assert synced == ["file", "directory"]
    assert path.read_bytes() == payload
    assert list(tmp_path.iterdir()) == [path]
    restored = keyfile.get_keypair()
    assert restored.ss58_address == keypair.ss58_address
    assert bytes(restored.hashed_public_key) == bytes(keypair.hashed_public_key)
    assert verify_mldsa_authorization(
        bytes(restored.sign_hashed(b"restored fixed account")),
        b"restored fixed account",
        bytes(keypair.hashed_descriptor),
        0,
        bytes(keypair.hashed_current_commitment),
    )


def test_declined_overwrite_preserves_existing_wallet(tmp_path, monkeypatch):
    path = tmp_path / "coldkey"
    path.write_bytes(b"previous wallet")
    monkeypatch.setattr("sys.stdin.isatty", lambda: False)
    with pytest.raises(FileExistsError):
        Keyfile(path)._write_data(b"new wallet", overwrite=False)
    assert path.read_bytes() == b"previous wallet"
    assert list(tmp_path.iterdir()) == [path]
