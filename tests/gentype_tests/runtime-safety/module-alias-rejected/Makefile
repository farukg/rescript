SHELL = /bin/bash

test:
	@output="$$(mktemp)"; \
	if yarn build >"$$output" 2>&1; then \
		cat "$$output"; \
		rm -f "$$output"; \
		echo "unsafe explicit genType module alias unexpectedly compiled"; \
		false; \
	fi; \
	cat "$$output"; \
	rg -q "genType runtime safety: module alias 'RuntimeAlias' cannot export runtime values" "$$output"; \
	rm -f "$$output"

clean:
	yarn clean

.DEFAULT_GOAL := test

.PHONY: clean test
