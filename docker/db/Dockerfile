FROM postgres:18-alpine

RUN apk add --no-cache --virtual .build-deps \
        build-base \
        git \
        postgresql-dev \
    && git clone --depth 1 https://github.com/ibrahimkarimeddin/postgres-extensions-pg_ttl.git /tmp/pg_ttl_index \
    && cd /tmp/pg_ttl_index \
    && make clean && make && make install \
    && cd / && rm -rf /tmp/pg_ttl_index \
    && apk del .build-deps

COPY init-ttl-worker.sh /docker-entrypoint-initdb.d/
