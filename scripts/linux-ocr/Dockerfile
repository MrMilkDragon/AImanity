ARG OCR_BASE
FROM ${OCR_BASE} AS factory
COPY bootstrap-ca.deb /tmp/bootstrap-ca.deb
RUN dpkg-deb -x /tmp/bootstrap-ca.deb /tmp/bootstrap-ca && mkdir -p /etc/ssl/certs && cat /tmp/bootstrap-ca/usr/share/ca-certificates/mozilla/*.crt > /etc/ssl/certs/ca-certificates.crt && rm -rf /tmp/bootstrap-ca /tmp/bootstrap-ca.deb
COPY ubuntu.sources /etc/apt/sources.list.d/ubuntu.sources
RUN apt-get update --error-on=any && apt-get install -y --no-install-recommends ca-certificates tesseract-ocr tesseract-ocr-eng tesseract-ocr-chi-sim && rm -rf /var/lib/apt/lists/*
COPY release-tools /tmp/release-tools
RUN /tmp/release-tools linux-ocr collect /out
FROM scratch AS export
COPY --from=factory /out/ /
