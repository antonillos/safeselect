FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --no-install-recommends --yes openssh-server socat \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/bash demo \
    && mkdir -p /run/sshd /home/demo/.ssh \
    && chown -R demo:demo /home/demo/.ssh \
    && chmod 700 /home/demo/.ssh

COPY sshd_config /etc/ssh/sshd_config.d/safeselect-demo.conf

EXPOSE 22

CMD ["/bin/sh", "-c", "socat TCP-LISTEN:5432,fork,reuseaddr TCP:postgres:5432 & exec /usr/sbin/sshd -D -e"]
