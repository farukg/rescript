SHELL = /bin/bash

test:
	yarn build
	yarn typecheck
	node --experimental-strip-types assert-runtime.mjs
	@rg -q '^export function increment\\(' src/Callable.gen.ts \
		|| { echo "missing TDZ-safe function declaration"; false; }

clean:
	yarn clean

.DEFAULT_GOAL := test

.PHONY: clean test
