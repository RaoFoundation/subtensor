"""Subnet services: reading a published service and calling it.

Covers `bittensor.services` and `btcli service`.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import os
import socket
import threading
import time
import urllib.parse
import urllib.request
from pathlib import Path

import pytest

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

    def __call__(self, method, url, headers, data, local=False, limit=None, timeout=None):
        self.sent.append((method, url, dict(headers), data))
        page = self.pages.get(url.split("?")[0] if url not in self.pages else url)
        if callable(page):  # answers by the request's headers
            page = page(headers)
        if isinstance(page, Response):
            return page
        if page is None:
            return Response(404, {}, b"")
        return Response(200, {"content-type": "application/json"}, json.dumps(page).encode())


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

    def test_names_the_service_when_a_subnet_has_several(self, web):
        with pytest.raises(ServiceError, match="2 services; name one"):
            load({ALICE_HOT: DOC_URL, BOB_HOT: DOC_URL})
        assert load({ALICE_HOT: DOC_URL, BOB_HOT: DOC_URL}, ALICE_HOT).hotkey == ALICE_HOT
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

    @pytest.mark.parametrize(
        "entries,found",
        [
            ([{"type": "mcp", "endpoint": "https://svc.example/mcp"}], "no OpenAPI spec"),
            (
                [
                    {"type": "openapi", "endpoint": SPEC_URL},
                    {"type": "http", "endpoint": "https://svc.example", "spec": SPEC_URL},
                ],
                "2 OpenAPI specs",
            ),
        ],
        ids=["none", "two"],
    )
    def test_the_document_names_one_spec(self, web, entries, found):
        web.pages[DOC_URL] = document(services=entries)
        with pytest.raises(ServiceError, match=found):
            load()

    def test_the_spec_must_be_openapi_3(self, web):
        web.pages[SPEC_URL] = {"swagger": "2.0", "paths": {}}
        with pytest.raises(ServiceError, match=r"not an OpenAPI 3 spec \(swagger '2.0'\)"):
            load()

    def test_without_a_btauth_scheme_every_operation_not_marked_open_is_signed(self, web):
        undeclared = spec()
        undeclared["components"]["securitySchemes"] = {"key": {"type": "http"}}
        web.pages[SPEC_URL] = undeclared
        auth = {op.id: op.auth for op in load().operations.values()}
        assert (auth["listModels"], auth["listJobs"], auth["admin"], auth["maybe"]) == (
            "open",
            "hotkey",
            "hotkey",
            "optional",
        )

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
        }
        assert {name for name, kind in auth.items() if kind == "open"} == {"listModels", "maybe"}
        assert set(map(str, service.skipped)) == {
            "GET /admin (admin) needs credentials other than btauth/1",
            "GET /both (both) needs credentials other than btauth/1",
            "POST /upload (upload) takes a body that is multipart/form-data, not JSON",
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

    @pytest.mark.parametrize(
        "path,operation,reason",
        [
            (
                "/clash",
                {
                    "operationId": "clash",
                    "parameters": [{"name": "model", "in": "query"}],
                    "requestBody": {
                        "content": {
                            "application/json": {"schema": {"$ref": "#/components/schemas/Job"}}
                        }
                    },
                },
                "has two inputs named 'model'",
            ),
            ("/again", {"operationId": "listJobs"}, "repeats the operationId 'listJobs'"),
            (
                "/file",
                {
                    "operationId": "file",
                    "requestBody": {"content": {"multipart/form-data": {"schema": {}}}},
                },
                "takes a body that is multipart/form-data, not JSON",
            ),
            (
                "/cookie",
                {
                    "operationId": "cookie",
                    "parameters": [{"name": "s", "in": "cookie", "required": True}],
                },
                "needs 's' in 'cookie'",
            ),
        ],
        ids=["clash", "repeat", "not-json", "cookie"],
    )
    def test_operations_that_break_the_standard_are_skipped_with_why(
        self, web, path, operation, reason
    ):
        broken = spec()
        broken["paths"][path] = {"post": operation}
        web.pages[SPEC_URL] = broken
        service = load()
        assert f"POST {path} ({operation['operationId']}) {reason}" in map(str, service.skipped)
        assert "listModels" in service.operations

    def test_operations_named_alike_are_skipped(self, web):
        alike = spec()
        alike["paths"]["/user"] = {"get": {"operationId": "getUser", "security": []}}
        alike["paths"]["/user2"] = {"get": {"operationId": "get_user", "security": []}}
        web.pages[SPEC_URL] = alike
        service = load()
        assert "getUser" not in service.operations and "get_user" not in service.operations
        assert "GET /user2 (get_user) shares the name 'get-user'" in map(str, service.skipped)

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
        odd["paths"]["/frag#x"] = {"get": {"operationId": "frag", "security": []}}
        odd["paths"]["/models"]["get"]["parameters"] = 5
        web.pages[SPEC_URL] = odd
        service = load()
        assert "odd" not in service.operations and "frag" not in service.operations
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

    def test_an_object_parameter_is_not_offered(self, web):
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
        assert [i.name for i in service.operations["filter"].inputs] == ["q"]
        assert "GET /deep (deep) takes 'o' as an object in the query" in map(str, service.skipped)

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

    def test_a_path_parameter_must_be_declared(self, web):
        undeclared = spec()
        undeclared["paths"]["/items/{item}"] = {"get": {"operationId": "item", "security": []}}
        web.pages[SPEC_URL] = undeclared
        assert "GET /items/{item} (item) does not declare its path parameter 'item'" in map(
            str, load().skipped
        )

    def test_a_skipped_operation_says_why_when_called(self, web):
        with pytest.raises(ServiceError, match=r"'admin' is not offered by Example: GET /admin"):
            load().operation("admin")

    def test_an_operation_is_a_tool_definition(self, web):
        service = load()
        tool = service.tool("createJob")
        assert tool["name"] == "create-job" and tool == service.tool("create-job")
        schema = tool["input_schema"]
        assert schema["type"] == "object" and schema["required"] == ["model"]
        assert schema["properties"]["model"]["description"] == "Model id"
        assert "$ref" not in json.dumps(schema)
        assert [t["name"] for t in service.tools()] == [
            op.command for op in service.operations.values()
        ]

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
        with pytest.raises(ServiceError) as error:
            load({ALICE_HOT: DOC_URL, BOB_HOT: DOC_URL})
        assert error.value.code == "ambiguous"
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

    def test_a_balance_for_another_hotkey_is_refused(self, web):
        service = load()
        web.pages[f"{BALANCE_URL}?hotkey={BOB_HOT}"] = {"hotkey": ALICE_HOT, "remaining_usd": "9"}
        with pytest.raises(ServiceError, match=r"is .*'s, not"):
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

    def test_a_response_that_ends_early_is_refused(self):
        class Reply:
            def __init__(self):
                self.headers = {"content-length": "10"}

            def read(self, limit):
                return b"abc"

        with pytest.raises(ServiceError, match="ended early"):
            services._read(Reply(), 1 << 20, "https://svc.example")

    def test_a_response_over_the_limit_is_refused(self):
        class Reply:
            def read(self, limit):
                return b"x" * limit

        with pytest.raises(ServiceError, match="over"):
            services._read(Reply(), 1 << 20, "https://svc.example")


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

    def test_list_shows_every_service_and_why_one_cannot_be_used(self, cli, monkeypatch):
        from bittensor.cli import context as cli_context

        substrate = every_service({ALICE_HOT: DOC_URL, BOB_HOT: SPEC_URL})
        monkeypatch.setattr(
            cli_context, "Client", lambda network, **_: Client(network, substrate=substrate)
        )
        result = invoke("service", "list")
        assert result.exit_code == 0, result.output
        assert "Example" in result.output and "does not list netuid 1" in result.output
        records = json.loads(invoke("--json", "service", "list").output)
        mine = [r for r in records if r["hotkey"] == ALICE_HOT]
        assert [(r["name"], r["usable"]) for r in mine] == [("Example", True)]
        assert set(mine[0]["operations"]) == {"open", "optional", "hotkey"}
        assert [r["usable"] for r in records if r["url"] == SPEC_URL] == [False]
        assert [r["url"] for r in records if r["problem"]] == [SPEC_URL]

    def test_show_lists_the_operations(self, cli):
        result = invoke("--json", "service", "show", "--netuid", "1")
        assert result.exit_code == 0, result.output
        record = json.loads(result.output)
        assert record["api"] == "https://svc.example/api/v1"
        ops = {op["operation"]: op for op in record["operations"]}
        assert ops["list-models"]["auth"] == "open" and "input_schema" not in ops["list-models"]
        assert ops["create-job"]["required"] == ["model"]
        assert "limit" in ops["create-job"]["optional"]
        assert f"--service-hotkey {ALICE_HOT}" in record["pay"]
        full = json.loads(invoke("--json", "service", "show", "--netuid", "1", "--schemas").output)
        job = next(op for op in full["operations"] if op["operation"] == "create-job")
        assert job["input_schema"]["properties"]["model"]["type"] == "string"
        assert job["input_schema"]["required"] == ["model"]

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
        for text in ("--limit", "INTEGER", "--model", "Model id", "--tags", "--no-dry-run"):
            assert text in result.output
        record = json.loads(
            invoke("--json", "service", "call", "--netuid", "1", "create-job", "--help").output
        )
        assert record["operation"] == "create-job" and record["auth"] == "hotkey"
        assert record["input_schema"]["properties"]["model"]["type"] == "string"
        last = invoke("service", "call", "--netuid", "1", "create-job", "--help", "--json")
        assert json.loads(last.output) == record
        assert not [sent for sent in cli.sent if "/api/v1/" in sent[1]]

    def test_inputs_are_flags_typed_by_the_spec(self, cli, tmp_path):
        cli.pages["https://svc.example/api/v1/jobs"] = {"id": "j1"}
        model = tmp_path / "model.txt"
        model.write_text("from-file\n")
        result = invoke(
            "service", "call", "--netuid", "1", "create-job",
            "--limit", "2", "--model", f"@{model}", "--tags", "a", "--tags=b", "--dry-run",
        )  # fmt: skip
        assert result.exit_code == 0, result.output
        _, url, _, data = cli.sent[-1]
        assert url.endswith("/jobs?limit=2")
        assert json.loads(data) == {"model": "from-file", "tags": ["a", "b"], "dry_run": True}
        assert caller(cli.sent[-1], load()).hotkey_ss58 == wallet_hotkey()

    def test_btcli_options_and_inputs_never_clash(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = {"id": "j1"}
        result = invoke("service", "call", "--netuid", "1", "createJob", "--model", "--netuid")
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
            (["create-job"], "needs --model"),
            (["create-job", "--model", "m", "--limit", "two"], "--limit takes INTEGER"),
            (["create-job", "--model", "m", "--limit", "NaN"], "--limit takes INTEGER"),
            (["create-job", "--model", "m", "--model", "n"], "given twice"),
            (["create-job", "--model"], "needs a value"),
            (["create-job", "--no-model"], "--no-model takes no value"),
            (["create-job", "--inputs", "[1]"], "--inputs takes a JSON object"),
            (["create-job", "--inputs", '{"modle": "m"}'], "did you mean model"),
            (["create-job", "--model", "@/no/such/file"], "cannot read"),
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
            "node": {"cpu": {"model": "Xeon"}},
            "keys": [{"name": "laptop"}],
        }
        fields = invoke("service", "call", "--netuid", "1", "get-job", "--id", "j1")
        assert fields.exit_code == 0 and "done" in fields.output
        assert "laptop" in fields.output and "node.cpu.model  Xeon" in fields.output
        assert "note null" in " ".join(fields.output.split())
        cli.pages["https://svc.example/api/v1/jobs"] = [{"id": "j1", "gpu": "RTX"}]
        table = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert table.exit_code == 0 and "Jobs" in table.output and "RTX" in table.output
        raw = invoke("--json", "service", "call", "--netuid", "1", "list-jobs")
        answer = json.loads(raw.output)
        assert answer["body"] == [{"id": "j1", "gpu": "RTX"}]
        assert (answer["status"], answer["signed"]) == (200, wallet_hotkey())
        wide = {f"f{n}": n for n in range(10)} | {"id": "j9"}
        cli.pages["https://svc.example/api/v1/jobs"] = [wide]
        whole = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert "f9" in whole.output and "more field" not in whole.output
        cli.pages["https://svc.example/api/v1/jobs"] = [wide] * 4
        cut = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert "j9" in cut.output and "3 more fields" in cut.output and "--out" in cut.output
        cli.pages["https://svc.example/api/v1/jobs"] = []
        empty = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert empty.exit_code == 0 and "none" in empty.output

    def test_call_an_open_operation_needs_no_wallet_key(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = {"data": ["m"]}
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0, result.output
        assert http_auth.HEADER_SIGNATURE not in cli.sent[-1][2]

    def test_a_402_says_how_to_pay(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = Response(
            402, {"content-type": "application/json"}, b'{"error":"insufficient_balance"}'
        )
        result = invoke("service", "call", "--netuid", "1", "list-jobs")
        assert result.exit_code == 1
        assert "HTTP 402 from Example" in result.output
        text = " ".join(result.output.split())
        assert f"btcli gamma spend --netuid 1 --service-hotkey {ALICE_HOT}" in text
        assert f"-w {_WALLET_NAME} -H default" in text

    def test_an_error_shows_the_message_the_service_sent(self, cli):
        cli.pages["https://svc.example/api/v1/jobs/j2"] = Response(
            404, {"content-type": "application/json"}, b'{"error":{"message":"no job j2"}}'
        )
        result = invoke("service", "call", "--netuid", "1", "get-job", "--id", "j2")
        assert result.exit_code == 1
        assert "HTTP 404 from Example" in result.output and "note: no job j2" in result.output

    def test_a_binary_response_goes_to_out(self, cli, tmp_path):
        cli.pages["https://svc.example/api/v1/models"] = Response(
            200, {"content-type": "video/mp4"}, b"\x00\x01"
        )
        out = tmp_path / "video.mp4"
        result = invoke("service", "call", "--netuid", "1", "--out", str(out), "list-models")
        assert result.exit_code == 0, result.output
        assert out.read_bytes() == b"\x00\x01"

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
        assert result.exit_code == 2
        assert "btcli's own options go before the operation" in " ".join(result.output.split())
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

    def test_an_untyped_input_takes_json_and_a_doubled_at_is_literal(self, cli):
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
        args = ["free", "--meta", '{"a": 1}', "--note", "@@home"]
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
        assert (error["status"], error["body"], error["note"]) == (503, {"detail": "down"}, "down")
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
        assert "btcli gamma spend --netuid 1" in " ".join(result.output.split())

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
        assert "wrote 8 bytes of image/png to" in text
        Path(text.rsplit(" to ", 1)[1].replace(" ", "")).unlink()

    def test_fields_that_flatten_to_one_name_print_as_json(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = {"a.b": 1, "a": {"b": 2}}
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 0 and json.loads(result.output) == {"a.b": 1, "a": {"b": 2}}

    def test_an_optional_operation_can_be_sent_unsigned(self, cli):
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

    def test_an_input_takes_the_specs_spelling_and_stdin(self, cli):
        cli.pages["https://svc.example/api/v1/jobs"] = {}
        args = ["create-job", "--model", "@-", "--dry_run"]
        result = runner.invoke(app, ["service", "call", "--netuid", "1", *args], input="m1\n")
        assert result.exit_code == 0, result.output
        assert json.loads(cli.sent[-1][3]) == {"model": "m1", "dry_run": True}
        typo = invoke("service", "call", "--netuid", "1", "create-job", "--modle", "m")
        assert typo.exit_code == 2 and "did you mean --model" in typo.output

    def test_an_open_operation_error_has_no_wallet_hint(self, cli):
        cli.pages["https://svc.example/api/v1/models"] = Response(401, {}, b"")
        result = invoke("service", "call", "--netuid", "1", "list-models")
        assert result.exit_code == 1 and "HTTP 401" in result.output
        assert "help:" not in result.output
