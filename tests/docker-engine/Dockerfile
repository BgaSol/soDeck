FROM docker:28-dind
RUN apk add --no-cache openssh && (getent group docker || addgroup -S docker) && adduser -D -s /bin/sh -G docker ops && echo 'ops:fixture-password' | chpasswd && mkdir -p /run/sshd
RUN printf 'PasswordAuthentication yes\nPermitRootLogin no\nAllowUsers ops\nSubsystem sftp internal-sftp\n' > /etc/ssh/sshd_config
COPY start.sh /usr/local/bin/sodeck-test-start
RUN chmod 755 /usr/local/bin/sodeck-test-start
ENTRYPOINT ["/usr/local/bin/sodeck-test-start"]
