# Build configuration
CARGO ?= cargo
TARGET_DIR = release

# Installation paths
PREFIX ?= /usr
BINDIR ?= $(PREFIX)/bin
SYSTEMD_SYSTEM_UNIT_DIR ?= $(PREFIX)/lib/systemd/system
DBUS_SYSTEM_DIR ?= /etc/dbus-1/system.d

# Install commands
INSTALL := install
INSTALL_PROGRAM := $(INSTALL) -D -m 0755
INSTALL_DATA := $(INSTALL) -D -m 0644
INSTALL_DIR := $(INSTALL) -d -m 0755

# Binary names
BIN_DAEMON := chainsawd
BIN_CLI := chainsaw

# Asset files
SERVICE_FILE := chainsawd.service
DBUS_CONFIG := com.chainsaw.daemon.conf

# Build targets
TARGET_DAEMON := target/$(TARGET_DIR)/$(BIN_DAEMON)
TARGET_CLI := target/$(TARGET_DIR)/$(BIN_CLI)

.DEFAULT_GOAL := build

#=============================================================================
# Build targets
#=============================================================================

build:
	$(CARGO) build --release

check:
	$(CARGO) check --release
	$(CARGO) clippy --release -- -D warnings

#=============================================================================
# Installation targets
#=============================================================================

install:
	@echo "Installing binaries..."
	$(INSTALL_PROGRAM) "$(TARGET_DAEMON)" "$(DESTDIR)$(BINDIR)/$(BIN_DAEMON)"
	$(INSTALL_PROGRAM) "$(TARGET_CLI)" "$(DESTDIR)$(BINDIR)/$(BIN_CLI)"
	@echo "Installing systemd service..."
	$(INSTALL_DATA) "assets/$(SERVICE_FILE)" "$(DESTDIR)$(SYSTEMD_SYSTEM_UNIT_DIR)/$(SERVICE_FILE)"
	@echo "Installing D-Bus config..."
	$(INSTALL_DATA) "assets/$(DBUS_CONFIG)" "$(DESTDIR)$(DBUS_SYSTEM_DIR)/$(DBUS_CONFIG)"
ifeq ($(DESTDIR),)
	@echo "Reloading systemd daemon..."
	systemctl daemon-reload
	@if systemctl is-enabled --quiet $(SERVICE_FILE) 2>/dev/null; then \
		echo "Service already enabled."; \
	else \
		echo "Enabling service..."; \
		systemctl enable $(SERVICE_FILE); \
	fi
	@echo "Please reboot."
endif

.PHONY: build check install
