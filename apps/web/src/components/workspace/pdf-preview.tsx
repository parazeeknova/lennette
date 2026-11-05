/* eslint-disable jsx-a11y/prefer-tag-over-role -- PDF pages require a labelled drawing canvas, not an image element. */
import { GlobalWorkerOptions, getDocument } from "pdfjs-dist";
import type { PDFDocumentProxy, PDFPageProxy, RenderTask } from "pdfjs-dist";
// eslint-disable-next-line import/default -- Vite's ?url import exports the worker asset URL.
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { useEffect, useRef, useState } from "react";

import { button } from "./workspace-ui";

GlobalWorkerOptions.workerSrc = workerUrl;

const PdfPage = ({
  document,
  page,
}: {
  document: PDFDocumentProxy;
  page: number;
}) => {
  const canvas = useRef<HTMLCanvasElement>(null);
  const container = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(320);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!container.current) {
      return;
    }
    const observer = new ResizeObserver((entries) => {
      const next = entries[0]?.contentRect.width;
      if (next && next > 0) {
        setWidth(Math.floor(next));
      }
    });
    observer.observe(container.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    let disposed = false;
    let render: RenderTask | undefined;
    let pdfPage: PDFPageProxy | undefined;
    const draw = async () => {
      try {
        pdfPage = await document.getPage(page);
        if (disposed || !canvas.current) {
          return;
        }
        const base = pdfPage.getViewport({ scale: 1 });
        const scale = Math.min(
          (width * Math.min(window.devicePixelRatio || 1, 2)) / base.width,
          Math.sqrt(1_500_000 / (base.width * base.height))
        );
        const viewport = pdfPage.getViewport({ scale });
        const target = canvas.current;
        target.width = Math.ceil(viewport.width);
        target.height = Math.ceil(viewport.height);
        // Canvas-only rendering: no document scripts, annotations, forms or links.
        render = pdfPage.render({ canvas: target, viewport });
        await render.promise;
      } catch {
        if (!disposed) {
          setError(
            "This page could not be rendered. Download the original to read it."
          );
        }
      }
    };
    void draw();
    return () => {
      disposed = true;
      render?.cancel();
      pdfPage?.cleanup();
    };
  }, [document, page, width]);
  return (
    <div ref={container}>
      {error ? (
        <p role="alert" className="p-5 text-sm text-ink-soft">
          {error}
        </p>
      ) : (
        <canvas
          ref={canvas}
          role="img"
          aria-label={`PDF page ${page}`}
          className="h-auto w-full rounded-xl bg-white"
        />
      )}
    </div>
  );
};

const PdfPreview = ({ url }: { url: string }) => {
  const [document, setDocument] = useState<PDFDocumentProxy | null>(null);
  const [page, setPage] = useState(1);
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false;
    const loading = getDocument({
      url,
      maxImageSize: 4_000_000,
      disableAutoFetch: true,
      enableXfa: false,
      isOffscreenCanvasSupported: false,
    });
    const timer = setTimeout(() => {
      if (!disposed) {
        setError("PDF preview timed out. Download the original to read it.");
        void loading.destroy();
      }
    }, 30_000);
    const load = async () => {
      try {
        const pdf = await loading.promise;
        if (!disposed) {
          setDocument(pdf);
        }
      } catch {
        if (!disposed) {
          setError(
            "This PDF could not be previewed. Download the original to read it."
          );
        }
      }
      clearTimeout(timer);
    };
    void load();
    return () => {
      disposed = true;
      clearTimeout(timer);
      void loading.destroy();
    };
  }, [url]);
  if (error) {
    return (
      <p role="alert" className="text-sm text-ink-soft">
        {error}
      </p>
    );
  }
  if (!document) {
    return (
      <output className="block py-6 text-sm text-ink-soft">Opening PDF…</output>
    );
  }
  return (
    <div className="space-y-4">
      <nav
        aria-label="PDF pages"
        className="flex flex-wrap items-center justify-between gap-3"
      >
        <button
          type="button"
          className={button}
          disabled={page <= 1}
          onClick={() => setPage((p) => p - 1)}
        >
          Previous page
        </button>
        <span className="text-sm text-ink-soft">
          Page {page} of {document.numPages}
        </span>
        <button
          type="button"
          className={button}
          disabled={page >= document.numPages}
          onClick={() => setPage((p) => p + 1)}
        >
          Next page
        </button>
      </nav>
      <PdfPage key={page} document={document} page={page} />
    </div>
  );
};
export default PdfPreview;
