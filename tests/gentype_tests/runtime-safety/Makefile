SHELL = /bin/bash

test:
	make -C callable-cycle clean test
	make -C module-alias clean test
	make -C module-alias-rejected clean test

clean:
	make -C callable-cycle clean
	make -C module-alias clean
	make -C module-alias-rejected clean

.DEFAULT_GOAL := test

.PHONY: clean test
