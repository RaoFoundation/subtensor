"""Creating a wallet must not leave its private and public keyfiles split."""

import errno
import os
import stat
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest

from bittensor.keyfiles import Keyfile, Keypair
from bittensor.sp_core import CRYPTO_MLDSA65_FIXED, CRYPTO_SR25519
from bittensor.wallet import Wallet
from bittensor.wallets import create


def _files(wallet: Wallet, role: str):
    if role == "coldkey":
        return wallet.coldkey_file, wallet.coldkeypub_file
    return wallet.hotkey_file, wallet.hotkeypub_file


def _regenerate(wallet: Wallet, role: str, crypto: int, seed: int, overwrite: bool):
    getattr(wallet, f"regenerate_{role}")(
        seed=bytes([seed]) * 32,
        crypto_type=crypto,
        use_password=False,
        overwrite=overwrite,
    )


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
@pytest.mark.parametrize("crypto", [CRYPTO_SR25519, CRYPTO_MLDSA65_FIXED])
def test_declining_second_overwrite_preserves_both_keyfiles(tmp_path, monkeypatch, role, crypto):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, role, crypto, 31, overwrite=True)
    private, public = _files(wallet, role)
    previous = (Path(private.path).read_bytes(), Path(public.path).read_bytes())

    answers = iter(("yes", "no"))
    monkeypatch.setattr("sys.stdin.isatty", lambda: True)
    monkeypatch.setattr("builtins.input", lambda *_: next(answers))
    with pytest.raises(FileExistsError, match="refusing to overwrite"):
        _regenerate(wallet, role, crypto, 32, overwrite=False)

    assert (Path(private.path).read_bytes(), Path(public.path).read_bytes()) == previous
    assert private.get_keypair().ss58_address == public.get_keypair().ss58_address
    assert not list(Path(private.path).parent.glob(".wallet-backup.*"))


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
@pytest.mark.parametrize("crypto", [CRYPTO_SR25519, CRYPTO_MLDSA65_FIXED])
def test_second_publication_sync_failure_restores_both_keyfiles(
    tmp_path, monkeypatch, role, crypto
):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, role, crypto, 31, overwrite=True)
    private, public = _files(wallet, role)
    previous = (Path(private.path).read_bytes(), Path(public.path).read_bytes())
    sync = os.fsync
    directory_syncs = 0

    def fail_second_directory_sync(fd):
        nonlocal directory_syncs
        if stat.S_ISDIR(os.fstat(fd).st_mode):
            directory_syncs += 1
            if directory_syncs == 2:
                raise OSError(errno.EIO, "injected second publication failure")
        sync(fd)

    monkeypatch.setattr(os, "fsync", fail_second_directory_sync)
    with pytest.raises(OSError, match="injected second publication failure"):
        _regenerate(wallet, role, crypto, 32, overwrite=True)

    assert directory_syncs == 2
    assert (Path(private.path).read_bytes(), Path(public.path).read_bytes()) == previous
    assert private.get_keypair().ss58_address == public.get_keypair().ss58_address
    assert not list(Path(private.path).parent.glob(".wallet-backup.*"))


def test_new_private_file_race_rolls_back_public_without_clobbering_racer(tmp_path, monkeypatch):
    wallet = Wallet("pair", path=str(tmp_path))
    private, public = _files(wallet, "coldkey")
    private_path, public_path = Path(private.path), Path(public.path)
    publish_public = public._publish_staged

    def publish_then_race(tmp, *, overwrite, _on_publish=None):
        publish_public(tmp, overwrite=overwrite, _on_publish=_on_publish)
        private_path.write_bytes(b"created by another writer")

    monkeypatch.setattr(public, "_publish_staged", publish_then_race)
    with pytest.raises(FileExistsError):
        _regenerate(wallet, "coldkey", CRYPTO_SR25519, 31, overwrite=False)

    assert private_path.read_bytes() == b"created by another writer"
    assert not public_path.exists()
    assert not list(private_path.parent.glob(".wallet-backup.*"))


def test_password_env_failure_restores_both_keyfiles(tmp_path, monkeypatch):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, "coldkey", CRYPTO_SR25519, 31, overwrite=True)
    private, public = _files(wallet, "coldkey")
    previous = (Path(private.path).read_bytes(), Path(public.path).read_bytes())
    monkeypatch.setenv(private.env_var_name(), "old password")
    private.should_save_to_env = False

    def fail_save(_password):
        raise OSError(errno.EIO, "injected password environment failure")

    monkeypatch.setattr(private, "save_password_to_env", fail_save)
    with pytest.raises(OSError, match="injected password environment failure"):
        wallet.regenerate_coldkey(
            seed=bytes([32]) * 32,
            overwrite=True,
            coldkey_password="correct horse battery staple",
            save_coldkey_to_env=True,
        )

    assert (Path(private.path).read_bytes(), Path(public.path).read_bytes()) == previous
    assert os.environ[private.env_var_name()] == "old password"
    assert private.should_save_to_env is False
    assert not list(Path(private.path).parent.glob(".wallet-backup.*"))


def test_backup_cleanup_failure_does_not_report_keypair_failure(tmp_path, monkeypatch, caplog):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, "coldkey", CRYPTO_SR25519, 31, overwrite=True)
    rmdir = Path.rmdir

    def fail_cleanup(path):
        if path.name.startswith(".wallet-backup."):
            raise OSError(errno.EIO, "injected cleanup failure")
        return rmdir(path)

    monkeypatch.setattr(Path, "rmdir", fail_cleanup)
    _regenerate(wallet, "coldkey", CRYPTO_SR25519, 32, overwrite=True)

    private, public = _files(wallet, "coldkey")
    assert private.get_keypair().ss58_address == public.get_keypair().ss58_address
    assert "empty wallet backup directory could not be removed" in caplog.text
    assert all(
        not list(path.iterdir()) for path in Path(private.path).parent.glob(".wallet-backup.*")
    )


def test_paired_write_refuses_existing_keyfile_symlink(tmp_path):
    wallet = Wallet("pair", path=str(tmp_path))
    private, public = _files(wallet, "coldkey")
    private_path, public_path = Path(private.path), Path(public.path)
    private_path.parent.mkdir(parents=True)
    victim = tmp_path / "other-wallet-key"
    victim.write_bytes(b"untouched")
    private_path.symlink_to(victim)

    with pytest.raises(ValueError, match="cannot be symlinks"):
        _regenerate(wallet, "coldkey", CRYPTO_SR25519, 31, overwrite=True)

    assert private_path.is_symlink()
    assert victim.read_bytes() == b"untouched"
    assert not public_path.exists()


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_public_only_refusal_keeps_cached_and_persisted_identity(tmp_path, monkeypatch, role):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, role, CRYPTO_SR25519, 31, overwrite=True)
    _private, public = _files(wallet, role)
    old_address = public.get_keypair().ss58_address
    new_address = Keypair.create_from_seed(bytes([32]) * 32, CRYPTO_SR25519).ss58_address
    monkeypatch.setattr("sys.stdin.isatty", lambda: False)

    with pytest.raises(FileExistsError):
        getattr(wallet, f"regenerate_{role}pub")(ss58_address=new_address, overwrite=False)

    assert getattr(wallet, f"{role}pub").ss58_address == old_address
    assert public.get_keypair().ss58_address == old_address


def test_high_level_create_writes_fresh_cold_and_hot_pairs(tmp_path):
    wallet = create(
        name="pair", path=str(tmp_path), use_password=False, on_mnemonic=lambda *_: None
    )
    assert (
        wallet.coldkey_file.get_keypair().ss58_address
        == wallet.coldkeypub_file.get_keypair().ss58_address
    )
    assert (
        wallet.hotkey_file.get_keypair().ss58_address
        == wallet.hotkeypub_file.get_keypair().ss58_address
    )


def test_high_level_create_refuses_existing_files_before_any_overwrite(tmp_path, monkeypatch):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, "coldkey", CRYPTO_SR25519, 31, overwrite=True)
    _regenerate(wallet, "hotkey", CRYPTO_SR25519, 32, overwrite=True)
    files = (wallet.coldkey_file, wallet.coldkeypub_file, wallet.hotkey_file, wallet.hotkeypub_file)
    previous = tuple(Path(keyfile.path).read_bytes() for keyfile in files)
    answers = iter(("yes", "yes", "no"))
    prompts = []
    monkeypatch.setattr("sys.stdin.isatty", lambda: True)
    monkeypatch.setattr("builtins.input", lambda prompt: prompts.append(prompt) or next(answers))

    with pytest.raises(FileExistsError, match="refusing to overwrite"):
        create(name="pair", path=str(tmp_path), use_password=False, overwrite=False)

    assert tuple(Path(keyfile.path).read_bytes() for keyfile in files) == previous
    assert len(prompts) == 3  # cold approvals and the refused hotkey, all before writes


def test_high_level_create_preapproves_four_interactive_overwrites(tmp_path, monkeypatch):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, "coldkey", CRYPTO_SR25519, 31, overwrite=True)
    _regenerate(wallet, "hotkey", CRYPTO_SR25519, 32, overwrite=True)
    files = (wallet.coldkey_file, wallet.coldkeypub_file, wallet.hotkey_file, wallet.hotkeypub_file)
    previous = tuple(Path(keyfile.path).read_bytes() for keyfile in files)
    prompts = []
    monkeypatch.setattr("sys.stdin.isatty", lambda: True)
    monkeypatch.setattr("builtins.input", lambda prompt: prompts.append(prompt) or "yes")

    created = create(
        name="pair",
        path=str(tmp_path),
        use_password=False,
        overwrite=False,
        on_mnemonic=lambda *_: None,
    )

    assert len(prompts) == 4
    assert tuple(Path(keyfile.path).read_bytes() for keyfile in files) != previous
    assert (
        created.coldkey_file.get_keypair().ss58_address
        == created.coldkeypub_file.get_keypair().ss58_address
    )
    assert (
        created.hotkey_file.get_keypair().ss58_address
        == created.hotkeypub_file.get_keypair().ss58_address
    )


def test_concurrent_overwrites_publish_complete_pairs(tmp_path, monkeypatch):
    first = Wallet("pair", path=str(tmp_path))
    second = Wallet("pair", path=str(tmp_path))
    _regenerate(first, "coldkey", CRYPTO_SR25519, 30, overwrite=True)
    published_first = threading.Event()
    release_first = threading.Event()
    published_second = threading.Event()
    first_thread = []
    publish = Keyfile._publish_staged
    public_path = Path(first.coldkeypub_file.path)

    def pause_after_public(self, tmp, *, overwrite, _on_publish=None):
        result = publish(self, tmp, overwrite=overwrite, _on_publish=_on_publish)
        if Path(self.path) == public_path:
            if threading.get_ident() == first_thread[0]:
                published_first.set()
                assert release_first.wait(timeout=5)
            else:
                published_second.set()
        return result

    monkeypatch.setattr(Keyfile, "_publish_staged", pause_after_public)

    def write_first():
        first_thread.append(threading.get_ident())
        _regenerate(first, "coldkey", CRYPTO_SR25519, 31, overwrite=True)

    with ThreadPoolExecutor(max_workers=2) as pool:
        first_future = pool.submit(write_first)
        assert published_first.wait(timeout=5)
        second_future = pool.submit(_regenerate, second, "coldkey", CRYPTO_SR25519, 32, True)
        try:
            assert not published_second.wait(timeout=0.1)
        finally:
            release_first.set()
        first_future.result(timeout=5)
        second_future.result(timeout=5)

    assert (
        first.coldkey_file.get_keypair().ss58_address
        == first.coldkeypub_file.get_keypair().ss58_address
    )
    assert published_second.is_set()


def test_high_level_create_hot_write_failure_restores_all_four_keyfiles(tmp_path, monkeypatch):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, "coldkey", CRYPTO_SR25519, 31, overwrite=True)
    _regenerate(wallet, "hotkey", CRYPTO_SR25519, 32, overwrite=True)
    files = (wallet.coldkey_file, wallet.coldkeypub_file, wallet.hotkey_file, wallet.hotkeypub_file)
    previous = tuple(Path(keyfile.path).read_bytes() for keyfile in files)
    publish = Keyfile._publish_staged

    def fail_hot_private(self, tmp, *, overwrite, _on_publish=None):
        if Path(self.path) == Path(wallet.hotkey_file.path):
            raise OSError(errno.EIO, "injected hotkey publication failure")
        return publish(self, tmp, overwrite=overwrite, _on_publish=_on_publish)

    monkeypatch.setattr(Keyfile, "_publish_staged", fail_hot_private)
    with pytest.raises(OSError, match="injected hotkey publication failure"):
        create(
            name="pair",
            path=str(tmp_path),
            use_password=False,
            overwrite=True,
            on_mnemonic=lambda *_: None,
        )

    assert tuple(Path(keyfile.path).read_bytes() for keyfile in files) == previous
    assert not list(Path(wallet.coldkey_file.path).parent.glob(".wallet-create-backup.*"))


def test_high_level_create_hot_failure_removes_new_cold_pair(tmp_path, monkeypatch):
    wallet = Wallet("pair", path=str(tmp_path))
    publish = Keyfile._publish_staged

    def fail_hot_private(self, tmp, *, overwrite, _on_publish=None):
        if Path(self.path) == Path(wallet.hotkey_file.path):
            raise OSError(errno.EIO, "injected hotkey publication failure")
        return publish(self, tmp, overwrite=overwrite, _on_publish=_on_publish)

    monkeypatch.setattr(Keyfile, "_publish_staged", fail_hot_private)
    with pytest.raises(OSError, match="injected hotkey publication failure"):
        create(
            name="pair",
            path=str(tmp_path),
            use_password=False,
            on_mnemonic=lambda *_: None,
        )

    for keyfile in (
        wallet.coldkey_file,
        wallet.coldkeypub_file,
        wallet.hotkey_file,
        wallet.hotkeypub_file,
    ):
        assert not Path(keyfile.path).exists()
    assert not list(Path(wallet.coldkey_file.path).parent.glob(".wallet-create-backup.*"))


def test_high_level_create_hot_failure_restores_cold_env_and_cache(tmp_path, monkeypatch):
    wallet = Wallet("pair", path=str(tmp_path))
    _regenerate(wallet, "coldkey", CRYPTO_SR25519, 31, overwrite=True)
    _regenerate(wallet, "hotkey", CRYPTO_SR25519, 32, overwrite=True)
    files = (wallet.coldkey_file, wallet.coldkeypub_file, wallet.hotkey_file, wallet.hotkeypub_file)
    previous = tuple(Path(keyfile.path).read_bytes() for keyfile in files)
    env_name = wallet.coldkey_file.env_var_name()
    monkeypatch.setenv(env_name, "old password")
    create_cold = Wallet.create_new_coldkey
    captured = []

    def create_cold_and_mutate_env(self, *args, **kwargs):
        result = create_cold(self, *args, **kwargs)
        self.coldkey_file.should_save_to_env = True
        os.environ[env_name] = "new password"
        captured.append(self)
        return result

    def fail_hot(_self, *args, **kwargs):
        raise OSError(errno.EIO, "injected hotkey creation failure")

    monkeypatch.setattr(Wallet, "create_new_coldkey", create_cold_and_mutate_env)
    monkeypatch.setattr(Wallet, "create_new_hotkey", fail_hot)
    with pytest.raises(OSError, match="injected hotkey creation failure"):
        create(
            name="pair",
            path=str(tmp_path),
            use_password=False,
            overwrite=True,
            on_mnemonic=lambda *_: None,
        )

    assert tuple(Path(keyfile.path).read_bytes() for keyfile in files) == previous
    assert os.environ[env_name] == "old password"
    assert captured[0].coldkey_file.should_save_to_env is False
    assert captured[0]._coldkey is None
    assert captured[0]._coldkeypub is None


def test_high_level_create_counts_dangling_symlink_as_existing(tmp_path):
    wallet = Wallet("pair", path=str(tmp_path))
    hot_path = Path(wallet.hotkey_file.path)
    hot_path.parent.mkdir(parents=True)
    hot_path.symlink_to(tmp_path / "missing-target")

    with pytest.raises(ValueError, match="cannot be symlinks"):
        create(
            name="pair",
            path=str(tmp_path),
            use_password=False,
            overwrite=False,
            on_mnemonic=lambda *_: None,
        )

    assert hot_path.is_symlink()
    assert not Path(wallet.coldkey_file.path).exists()
    assert not Path(wallet.coldkeypub_file.path).exists()
