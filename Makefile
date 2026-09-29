.PHONY: install
install:
	pnpm install

.PHONY: dev
dev: install
	pnpm tauri dev

.PHONY: dev-server
dev-server:
	cargo run -p overhear-server -- --port 4747 --no-auth --graphiql

.PHONY: dev-web
dev-web: install
	pnpm dev

.PHONY: build
build: install
	cargo build --workspace
	pnpm build

.PHONY: test
test:
	cargo test --workspace
	npx tsc --noEmit

.PHONY: fmt
fmt:
	nix fmt

.PHONY: clean
clean:
	cargo clean
	rm -rf node_modules dist
