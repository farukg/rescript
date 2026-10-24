SHELL = /bin/bash

test:
	yarn build
	yarn typecheck
	node --experimental-strip-types assert-runtime.mjs
	@! rg -q 'AliasConsumerJS\\.RuntimeAlias' src/AliasConsumer.gen.ts \
		|| { echo "genType emitted a non-existent runtime module alias"; false; }

clean:
	yarn clean

.DEFAULT_GOAL := test

.PHONY: clean test
