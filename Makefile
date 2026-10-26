# W0rkTree developer tasks. Works with GNU make on Linux, macOS and Windows
# (Git Bash / MSYS2 make). On Windows without make, use scripts/*.ps1.

SHELL := bash
.SHELLFLAGS := -euo pipefail -c
.DEFAULT_GOAL := help

ifeq ($(OS),Windows_NT)
EXE := .exe
else
EXE :=
endif

SERVER_DIR := services/server-go
SERVER_BIN := target/go/worktree-server$(EXE)
VERSION    ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)
COMMIT     ?= $(shell git rev-parse HEAD 2>/dev/null || echo unknown)
GO_LDFLAGS := -s -w -X main.version=$(VERSION) -X main.commit=$(COMMIT)
IMAGE      ?= worktree-server:$(VERSION)

.PHONY: help build build-rust build-go build-web build-admin release \
        test test-rust test-go test-admin lint lint-rust lint-go lint-web lint-admin \
        fmt fmt-check proto e2e dev ci node_modules docker docker-web compose-up compose-down clean

help: ## Show this help
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z_-]+:.*## / {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# ------------------------------------------------------------------ build
build: build-rust build-go build-admin ## Build Rust workspace, Go server and admin panel

build-rust: ## cargo build --workspace
	cargo build --workspace

build-go: ## Build the Go server into target/go/
	cd $(SERVER_DIR) && go build -trimpath -ldflags "$(GO_LDFLAGS)" -o "$(CURDIR)/$(SERVER_BIN)" .

build-web: node_modules ## Build apps/web (Next.js)
	npm run build --workspace=apps/web

build-admin: node_modules ## Build apps/admin (Vite) into apps/admin/dist
	npm run build --workspace=apps/admin

release: ## Optimized binaries (wt, worktree-bg, worktree-server)
	cargo build --release -p worktree-cli -p worktree-bg
	cd $(SERVER_DIR) && CGO_ENABLED=0 go build -trimpath -ldflags "$(GO_LDFLAGS)" -o "$(CURDIR)/$(SERVER_BIN)" .

node_modules:
	@[ -d node_modules ] || npm ci --no-audit --no-fund

# ------------------------------------------------------------------ test
test: test-rust test-go test-admin ## Run all unit tests

test-rust: ## cargo test --workspace
	cargo test --workspace

test-go: ## go test ./...
	cd $(SERVER_DIR) && go test -count=1 ./...

test-admin: node_modules ## apps/admin tests
	npm run test --workspace=apps/admin --if-present

e2e: ## End-to-end suite (scripts/e2e.sh)
	bash scripts/e2e.sh

# ------------------------------------------------------------------ lint / fmt
lint: lint-rust lint-go lint-web lint-admin ## All linters

lint-rust: ## rustfmt --check + clippy -D warnings
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

lint-go: ## gofmt check + go vet (+ golangci-lint if installed)
	@out="$$(cd $(SERVER_DIR) && gofmt -l .)"; if [ -n "$$out" ]; then echo "gofmt needed on:"; echo "$$out"; exit 1; fi
	cd $(SERVER_DIR) && go vet ./...
	@if command -v golangci-lint >/dev/null 2>&1; then cd $(SERVER_DIR) && golangci-lint run ./...; fi

lint-web: node_modules ## apps/web lint
	npm run lint --workspace=apps/web

lint-admin: node_modules ## apps/admin lint
	npm run lint --workspace=apps/admin --if-present

fmt: ## Format Rust and Go sources
	cargo fmt --all
	cd $(SERVER_DIR) && gofmt -w .

proto: ## Regenerate Go gRPC bindings from sync.proto
	bash scripts/gen-proto.sh

ci: ## Local CI pipeline (scripts/ci.sh; pass ARGS="--rust -k")
	bash scripts/ci.sh $(ARGS)

# ------------------------------------------------------------------ dev
dev: ## Go server (:7420) + admin Vite dev server (:5173)
	bash scripts/dev.sh

# ------------------------------------------------------------------ docker
docker: ## Build the server image (deploy/docker/Dockerfile)
	docker build -f deploy/docker/Dockerfile --build-arg VERSION=$(VERSION) --build-arg COMMIT=$(COMMIT) -t $(IMAGE) .

docker-web: ## Build the web image (apps/web/Dockerfile)
	docker build -f apps/web/Dockerfile -t worktree-web:$(VERSION) .

compose-up: ## docker compose up --build (needs WT_ADMIN_PASSWORD)
	docker compose up --build -d

compose-down: ## docker compose down
	docker compose down

clean: ## Remove build outputs
	cargo clean
	rm -rf target/go apps/admin/dist apps/web/.next apps/web/.source .turbo
