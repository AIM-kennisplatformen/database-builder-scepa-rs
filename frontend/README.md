# SCEPA frontend

React operator UI for the manual new-document workflow. A PDF is sent through
automatic ingestion, then extracted and manual metadata are reviewed side by
side before the canonical model is published.

```bash
npm install
npm run dev
```

Vite serves the UI at `http://localhost:5173` and proxies `/api` to the SCEPA
API at `http://localhost:3000`. Set `VITE_API_URL` to use another API origin.

Create a production build with `npm run build`.
