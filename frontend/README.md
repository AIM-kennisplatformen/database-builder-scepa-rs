# SCEPA frontend

React operator UI for the manual new-document workflow. A PDF is sent through
automatic ingestion and TypeDB publication. Valid artifacts continue into the
shared update editor for optional corrections; invalid artifacts continue into
the repair picker and then reuse that same editor.

```bash
npm install
npm run dev
```

Vite serves the UI at `http://localhost:5173` and proxies `/api` to the SCEPA
API at `http://localhost:3000`. Set `VITE_API_URL` to use another API origin.

Create a production build with `npm run build`.
