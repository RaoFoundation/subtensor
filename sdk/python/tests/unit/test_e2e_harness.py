from tests.e2e import conftest as e2e_conftest


def test_localnet_image_prefers_current_env_name(monkeypatch):
    monkeypatch.setenv("LOCALNET_IMAGE", "localnet:current")
    monkeypatch.setenv("LOCALNET_IMAGE_NAME", "localnet:legacy")

    assert e2e_conftest._localnet_image() == "localnet:current"


def test_localnet_image_accepts_legacy_workflow_env_name(monkeypatch):
    monkeypatch.delenv("LOCALNET_IMAGE", raising=False)
    monkeypatch.setenv("LOCALNET_IMAGE_NAME", "localnet:legacy")

    assert e2e_conftest._localnet_image() == "localnet:legacy"
