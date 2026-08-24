# Lumen

<p align="center">
  <strong>A lightning-fast, lightweight reference manager built with Rust & GPUI</strong>
</p>

<p align="center">
  <a href="#core-features">Core Features</a> •
  <a href="#design-philosophy">Design Philosophy</a> •
  <a href="#known-issues">Known Issues</a> •
  <a href="#license">License</a>
</p>

---

Lumen is designed for researchers seeking a fast, minimalist, and responsive reference management and reading tool.

By rejecting web-wrapper runtimes and leveraging modern systems programming, Lumen is built from the ground up with **Rust** and the **GPUI** framework (the GPU-accelerated UI framework powering the Zed editor). It is dedicated to delivering a **lightning-fast, lightweight, and distraction-free** research workflow.

---

## Core Features

### GPU-Accelerated Native PDF Reader

- **Hardware Rasterization:** Pure native rendering engine powered by GPU hardware acceleration, delivering fluid 60+ FPS continuous scrolling, dynamic scaling, and rapid page-flipping.
- **Deep Document Interaction:** Smooth multi-page text selection, high-precision search with instant result navigation, and rich color-coded highlight/underline annotations.
- **Hierarchical Navigation:** Document outline tree, synchronized visual thumbnail strip with text preview, and quick bookmarking.

### Picture-in-Picture (PiP) Reading & Comparison

- **Floating Pin Windows:** Pin arbitrary figures, tables, formulas, or full pages as floating reference overlays while reading subsequent text.
- **Hardware-Smooth Scaling & Dynamic Re-rendering:** Interactive free dragging, resizing, and smooth hardware zooming with tiered high-resolution re-rendering for pixel-crisp clarity.
- **Cross-Location Comparison:** Keep crucial context (such as architecture diagrams or mathematical proofs) always in view without constantly scrolling back and forth.

### AI Research Assistant & Document Analysis

- **Context-Aware Dialogue:** Multi-turn conversational assistant with full paper context awareness, supporting customized system prompts and message rollbacks.
- **AI Summary & Note Generation:** One-click generation of structured abstracts, methodology breakdowns, and takeaway notes directly integrated into the literature note library.
- **Multi-Session Management:** Maintain multiple concurrent conversation sessions per literature item for exploring different research angles.

### Literature & Citation Management

- **Automated Metadata Extraction:** Instant bibliographic metadata parsing and completion from local PDF files, DOI identifiers, and arXiv links.
- **Multi-Level Organization:** Infinite-depth hierarchical collection tree, flexible multi-tagging, reading progress tracking, and full-text search across library metadata.
- **Manuscript-Ready Export:** Standardized citation exports supporting BibTeX (`.bib`), IEEE, Elsevier, and APA formats.

### Multi-Platform Cloud Synchronization

- **Hybrid Synchronization Architecture:** Independent decoupled synchronization between file attachments and relational metadata.
- **WebDAV Attachment Storage:** Connect private WebDAV endpoints or commercial cloud storage (Nutstore, InfiniCLOUD, Nextcloud, etc.) for cross-device PDF sync.
- **Relational Consistency Engine:** Multi-device relational database synchronization with deterministic conflict resolution to ensure data integrity.

### Integrated Translation & Native Cross-Platform

- **In-Reader Translation:** Zero-friction paragraph and text selection translation directly embedded into the document workspace.
- **Pure Native Distribution:** Lightweight distribution with zero Chromium/Node.js overhead across Windows (x64), macOS (Apple Silicon / Intel), and Linux (Debian/Ubuntu).

---

## Design Philosophy

- **Zero Perceptible Latency:** Native compilation with GPU rasterization ensures immediate response during navigation and reading.
- **Minimal Resource Footprint:** Low memory footprint and instant startup times.
- **Focused Simplicity:** Clean, distraction-free interface designed to keep focus strictly on reading and research.

---

## Known Issues

- **macOS PDF Render Memory Deallocation:** Due to how image and texture memory caching is handled in the upstream GPUI framework on macOS, rasterized PDF page memory may not be released immediately upon closing documents. This can cause higher memory consumption during extended multi-document reading sessions. We are actively tracking upstream developments and working on downstream memory eviction optimizations.

---

## License

This project is licensed under the MIT License.
