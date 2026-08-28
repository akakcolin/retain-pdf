/* Self-contained mupdf subset+write shim for rendering_writer.
 *
 * mupdf raises via setjmp/longjmp (fz_try/fz_always/fz_catch); without a try
 * boundary an error calls exit(EXIT_FAILURE) inside mupdf, killing the host
 * process. This shim replicates mupdf's exception macros around
 * `pdf_subset_fonts` + `pdf_write_document` and returns any error through the
 * same `mupdf_error_t**` contract mupdf-sys's own wrapper uses, so Rust gets
 * an error back instead of a dead process.
 *
 * Runs on an isolated context created with `fz_new_context_imp` (NOT
 * `mupdf_new_base_context`, which shares a global static lock array with
 * mupdf-sys's base context and would destroy it on drop). All mupdf symbols
 * resolve at link time from the mupdf-sys static libs rendering_writer
 * already links. The version string must equal FZ_VERSION ("1.27.2") or
 * `fz_new_context_imp` refuses to create the context.
 */

#include <setjmp.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

/* ---- opaque mupdf types (the shim only moves pointers) ---- */
typedef struct fz_context fz_context;
typedef struct fz_buffer fz_buffer;
typedef struct fz_stream fz_stream;
typedef struct fz_output fz_output;
typedef struct pdf_document pdf_document;
typedef struct pdf_write_options pdf_write_options;
typedef struct fz_alloc_context fz_alloc_context;
typedef struct fz_locks_context fz_locks_context;

/* Error contract mirroring mupdf-sys wrapper/internal.h: `int type; char
 * *message;` (16 bytes on 64-bit). mupdf_save_error mallocs and fills it. */
typedef struct mupdf_error
{
    int type;
    char *message;
} mupdf_error_t;

/* ---- exception jump buffer: sigjmp_buf on POSIX, jmp_buf on Windows ---- */
#if defined(_WIN32) && !defined(HAVE_SIGSETJMP)
typedef jmp_buf fz_jmp_buf;
#define MUPDF_SETJMP(BUF) setjmp(BUF)
#define MUPDF_LONGJMP(BUF, VAL) longjmp(BUF, VAL)
#else
typedef sigjmp_buf fz_jmp_buf;
#define MUPDF_SETJMP(BUF) sigsetjmp(BUF, 0)
#define MUPDF_LONGJMP(BUF, VAL) siglongjmp(BUF, VAL)
#endif

/* ---- exception state machine (error.c / context.h) ---- */
fz_jmp_buf *fz_push_try(fz_context *ctx);
int fz_do_try(fz_context *ctx);
int fz_do_always(fz_context *ctx);
int fz_do_catch(fz_context *ctx);
void fz_var_imp(void *var);
void mupdf_save_error(fz_context *ctx, mupdf_error_t **errptr);

#define MUPDF_TRY(ctx) if (!MUPDF_SETJMP(*fz_push_try(ctx))) if (fz_do_try(ctx)) do
#define MUPDF_ALWAYS(ctx) while (0); if (fz_do_always(ctx)) do
#define MUPDF_CATCH(ctx) while (0); if (fz_do_catch(ctx))
#define MUPDF_VAR(var) fz_var_imp((void *)&(var))

/* ---- context ---- */
fz_context *fz_new_context_imp(const fz_alloc_context *alloc,
                               const fz_locks_context *locks, size_t max_store,
                               const char *version);
void fz_drop_context(fz_context *ctx);

/* ---- buffers / streams / outputs ---- */
fz_buffer *fz_new_buffer(fz_context *ctx, size_t capacity);
fz_buffer *fz_new_buffer_from_shared_data(fz_context *ctx,
                                          const unsigned char *data,
                                          size_t size);
void fz_drop_buffer(fz_context *ctx, fz_buffer *buf);
size_t fz_buffer_storage(fz_context *ctx, fz_buffer *buf,
                         unsigned char **datap);
void *fz_malloc(fz_context *ctx, size_t size);
fz_stream *fz_open_buffer(fz_context *ctx, fz_buffer *buf);
void fz_drop_stream(fz_context *ctx, fz_stream *stm);
fz_output *fz_new_output_with_buffer(fz_context *ctx, fz_buffer *buf);
void fz_close_output(fz_context *ctx, fz_output *out);
void fz_drop_output(fz_context *ctx, fz_output *out);

/* ---- pdf ---- */
pdf_document *pdf_open_document_with_stream(fz_context *ctx,
                                            fz_stream *file);
void pdf_drop_document(fz_context *ctx, pdf_document *doc);
void pdf_subset_fonts(fz_context *ctx, pdf_document *doc, int pages_len,
                      const int *pages);
void pdf_write_document(fz_context *ctx, pdf_document *doc, fz_output *out,
                        const pdf_write_options *opts);

#define MUPDF_FZ_STORE_DEFAULT ((size_t)268435456)
#define MUPDF_FZ_VERSION "1.27.2"

fz_context *mupdf_clean_new_context(void)
{
    return fz_new_context_imp(NULL, NULL, MUPDF_FZ_STORE_DEFAULT,
                              MUPDF_FZ_VERSION);
}

void mupdf_clean_free(void *ptr)
{
    free(ptr);
}

/* Subset the fonts of `data` and write the document out through mupdf,
 * returning a malloc'd copy in *out / *out_size. Returns 0 on success and -1
 * on
 * error with *errptr set (when errptr is non-NULL). */
int mupdf_subset_and_write_bytes(fz_context *ctx, const unsigned char *data,
                                 size_t size, const pdf_write_options *pwo,
                                 unsigned char **out, size_t *out_size,
                                 mupdf_error_t **errptr)
{
    fz_buffer *in = NULL;
    fz_stream *stream = NULL;
    pdf_document *pdf = NULL;
    fz_buffer *buf = NULL;
    fz_output *outp = NULL;
    unsigned char *copy = NULL;
    size_t n = 0;

    MUPDF_VAR(in);
    MUPDF_VAR(stream);
    MUPDF_VAR(pdf);
    MUPDF_VAR(buf);
    MUPDF_VAR(outp);
    MUPDF_VAR(copy);
    MUPDF_VAR(n);

    *out = NULL;
    *out_size = 0;

    MUPDF_TRY(ctx)
    {
        /* Non-owning: the buffer wraps `data` without copying, and is dropped
         * in ALWAYS before this function returns, while the Rust slice is
         * still valid. */
        in = fz_new_buffer_from_shared_data(ctx, data, size);
        stream = fz_open_buffer(ctx, in);
        pdf = pdf_open_document_with_stream(ctx, stream);
        /* pages_len == 0 means "every page" (pdf-subset.c). */
        pdf_subset_fonts(ctx, pdf, 0, NULL);
        buf = fz_new_buffer(ctx, 8192);
        outp = fz_new_output_with_buffer(ctx, buf);
        pdf_write_document(ctx, pdf, outp, pwo);
        fz_close_output(ctx, outp);
        {
            unsigned char *raw = NULL;
            n = fz_buffer_storage(ctx, buf, &raw);
            copy = (unsigned char *)fz_malloc(ctx, n ? n : 1);
            memcpy(copy, raw, n);
            *out = copy;
            *out_size = n;
        }
    }
    MUPDF_ALWAYS(ctx)
    {
        if (outp)
            fz_drop_output(ctx, outp);
        if (stream)
            fz_drop_stream(ctx, stream);
        if (in)
            fz_drop_buffer(ctx, in);
        if (pdf)
            pdf_drop_document(ctx, pdf);
        if (buf)
            fz_drop_buffer(ctx, buf);
    }
    MUPDF_CATCH(ctx)
    {
        if (errptr)
            mupdf_save_error(ctx, errptr);
    }

    return (*out && *out_size) ? 0 : -1;
}
