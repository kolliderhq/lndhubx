#!/usr/bin/env bash

docker-compose up -d db
sleep 10
if docker-compose exec db psql -U postgres -lqt | grep -qw lndhubx; then
    echo "DB found"
else
    echo "DB does not exist. Creating..."
    docker-compose exec db psql -U postgres -c "CREATE DATABASE lndhubx;"
fi
docker-compose up
