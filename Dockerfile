# ---- Stage 1: Build Vue frontend ----
FROM node:20-slim AS frontend-builder
WORKDIR /build
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ .
RUN npm run build

# ---- Stage 2: Python runtime ----
FROM python:3.11-slim

WORKDIR /app

COPY backend/requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY backend/ .
COPY --from=frontend-builder /build/dist ./static

RUN mkdir -p /app/data
ENV CONFIG_PATH=/app/data/config.json
VOLUME /app/data

EXPOSE 8000

CMD ["python", "run.py"]
