APPLICATIONS_DIR ?= /Applications
export APPLICATIONS_DIR

.PHONY: install
install:
	bash packaging/macos/install.sh "$$APPLICATIONS_DIR"
