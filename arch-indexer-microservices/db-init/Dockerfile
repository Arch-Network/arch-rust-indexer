FROM postgres:15

# Copy initialization SQL files into the image
COPY . /db-init

# Runner script to apply all SQL files in lexicographic order with psql
COPY run.sh /run.sh
RUN chmod +x /run.sh

ENTRYPOINT ["/bin/bash", "/run.sh"]
