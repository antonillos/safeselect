FROM ubuntu:24.04@sha256:534baea6a22c03a63003dbc8dbe78fe34bc0d7e595d9a9dc9834884ff530eb55
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    bash ca-certificates curl git openjdk-17-jre-headless openssh-client sshpass \
    && rm -rf /var/lib/apt/lists/*
# SHA-256 values from the public asdf v0.20.0 release asset metadata.
RUN arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
      arm64) checksum=bbc1889886a9826ce3f57f56e4bae575767a4af3d35d649d62116ee14334e59a ;; \
      amd64) checksum=9c25e1af7cc4c9d59ff3736eba14fd000480c32929258f80d8c5a8b290ebee14 ;; \
      *) exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/asdf-vm/asdf/releases/download/v0.20.0/asdf-v0.20.0-linux-${arch}.tar.gz" -o /tmp/asdf.tar.gz \
    && printf '%s  /tmp/asdf.tar.gz\n' "$checksum" | sha256sum -c - \
    && tar xzf /tmp/asdf.tar.gz -C /usr/local/bin asdf && rm /tmp/asdf.tar.gz
RUN useradd --create-home --shell /bin/bash demo && mkdir -p /home/demo/workspace /home/demo/.ssh \
    && chown -R demo:demo /home/demo
COPY compass.json /home/demo/compass.json
USER demo
ENV PATH="/home/demo/.asdf/shims:/usr/local/bin:/usr/bin:/bin" TERM=xterm-256color
WORKDIR /home/demo/workspace
CMD ["sleep", "infinity"]
