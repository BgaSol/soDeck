FROM ubuntu:24.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openssh-server rsync sshpass python3 ca-certificates procps && rm -rf /var/lib/apt/lists/*
RUN useradd -m -s /bin/sh ops && echo 'ops:fixture-password' | chpasswd && mkdir -p /run/sshd /home/ops/.ssh && chmod 700 /home/ops/.ssh && chown ops:ops /home/ops/.ssh
RUN printf 'PasswordAuthentication yes\nPubkeyAuthentication yes\nPermitRootLogin no\nUsePAM no\n' > /etc/ssh/sshd_config.d/fixture.conf
COPY rsync-fixture.py /usr/local/bin/rsync
RUN chmod 755 /usr/local/bin/rsync
CMD ["/usr/sbin/sshd", "-D", "-e"]
