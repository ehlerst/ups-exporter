BINARY_NAME=ups-exporter
INSTALL_PATH=/usr/local/bin

all: build

build:
	cargo build --release

install: build
	cp target/release/$(BINARY_NAME) $(INSTALL_PATH)/$(BINARY_NAME)
	chmod +x $(INSTALL_PATH)/$(BINARY_NAME)

setup-service:
	cp ups-exporter.service /etc/systemd/system/
	systemctl daemon-reload
	systemctl enable ups-exporter
	systemctl restart ups-exporter

clean:
	cargo clean
