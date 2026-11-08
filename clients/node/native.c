/* The ABI is loaded dynamically so the addon can use debug or release builds. */
#include <node_api.h>
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct MDB_env MDB_env;
typedef struct MDB_txn MDB_txn;
typedef struct MDB_cursor MDB_cursor;
typedef unsigned MDB_dbi;
typedef struct { size_t mv_size; void *mv_data; } MDB_val;
#define MDB_NOTFOUND (-30798)
#define MDB_RDONLY 0x20000u
#define MDB_NOTLS 0x200000u
#define MDB_CREATE 0x40000u

#define API_LIST(X) \
  X(int, mdb_env_create, (MDB_env **)) \
  X(int, mdb_env_set_maxdbs, (MDB_env *, MDB_dbi)) \
  X(int, mdb_env_open, (MDB_env *, const char *, unsigned, unsigned)) \
  X(void, mdb_env_close, (MDB_env *)) \
  X(int, mdb_txn_begin, (MDB_env *, MDB_txn *, unsigned, MDB_txn **)) \
  X(int, mdb_txn_commit, (MDB_txn *)) \
  X(void, mdb_txn_abort, (MDB_txn *)) \
  X(int, ulmdb_dbi_open, (MDB_txn *, const char *, unsigned, unsigned, MDB_dbi *)) \
  X(int, mdb_get, (MDB_txn *, MDB_dbi, MDB_val *, MDB_val *)) \
  X(int, mdb_put, (MDB_txn *, MDB_dbi, MDB_val *, MDB_val *, unsigned)) \
  X(int, mdb_del, (MDB_txn *, MDB_dbi, MDB_val *, MDB_val *)) \
  X(int, mdb_cursor_open, (MDB_txn *, MDB_dbi, MDB_cursor **)) \
  X(int, mdb_cursor_get, (MDB_cursor *, MDB_val *, MDB_val *, unsigned)) \
  X(void, mdb_cursor_close, (MDB_cursor *)) \
  X(char *, mdb_strerror, (int)) \
  X(int, ulmdb_prefix_put, (MDB_txn *, MDB_dbi, const char *, const MDB_val *)) \
  X(int, ulmdb_prefix_get, (MDB_txn *, MDB_dbi, const char *, MDB_val *)) \
  X(int, ulmdb_prefix_delete, (MDB_txn *, MDB_dbi, const char *)) \
  X(int, ulmdb_longest_match, (MDB_txn *, MDB_dbi, const char *, MDB_val *, MDB_val *))
#define DECLARE_API(result, name, parameters) static result (*api_##name) parameters;
API_LIST(DECLARE_API)

/* No pointers are exposed to JavaScript except typed N-API externals. */
#define ENV_MAGIC UINT32_C(0x554c454e)
#define TXN_MAGIC UINT32_C(0x554c5458)
typedef struct {
  uint32_t magic;
  MDB_env *pointer;
  size_t transactions;
  size_t references;
  bool writer;
} env_handle;
typedef struct {
  uint32_t magic;
  MDB_txn *pointer;
  env_handle *owner;
  bool read_only;
} txn_handle;

static napi_value fail(napi_env env, const char *message) {
  napi_throw_error(env, NULL, message);
  return NULL;
}
static napi_value db_error(napi_env env, int code) {
  napi_value message, error, number;
  if (napi_create_string_utf8(env, api_mdb_strerror(code), NAPI_AUTO_LENGTH, &message) != napi_ok ||
      napi_create_error(env, NULL, message, &error) != napi_ok ||
      napi_create_int32(env, code, &number) != napi_ok ||
      napi_set_named_property(env, error, "code", number) != napi_ok) return fail(env, "Cannot construct database error");
  napi_throw(env, error);
  return NULL;
}
static napi_value undefined(napi_env env) {
  napi_value value;
  if (napi_get_undefined(env, &value) != napi_ok) return fail(env, "Cannot return undefined");
  return value;
}
static napi_value boolean(napi_env env, bool answer) {
  napi_value value;
  if (napi_get_boolean(env, answer, &value) != napi_ok) return fail(env, "Cannot return boolean");
  return value;
}
static napi_value null_value(napi_env env) {
  napi_value value;
  if (napi_get_null(env, &value) != napi_ok) return fail(env, "Cannot return null");
  return value;
}
static bool arguments(napi_env env, napi_callback_info info, size_t count, napi_value *args) {
  size_t supplied = count;
  if (napi_get_cb_info(env, info, &supplied, args, NULL, NULL) != napi_ok || supplied != count) {
    fail(env, "Invalid native argument count"); return false;
  }
  return true;
}
static char *string(napi_env env, napi_value value) {
  size_t size, copied;
  if (napi_get_value_string_utf8(env, value, NULL, 0, &size) != napi_ok) {
    fail(env, "Expected string"); return NULL;
  }
  char *result = malloc(size + 1);
  if (!result) { fail(env, "Out of memory"); return NULL; }
  if (napi_get_value_string_utf8(env, value, result, size + 1, &copied) != napi_ok ||
      copied != size || memchr(result, '\0', size)) {
    free(result); fail(env, "Expected string without NUL bytes"); return NULL;
  }
  return result;
}
static bool uint32(napi_env env, napi_value value, uint32_t *result) {
  if (napi_get_value_uint32(env, value, result) != napi_ok) {
    fail(env, "Expected integer"); return false;
  }
  return true;
}
static bool flag(napi_env env, napi_value value, bool *result) {
  if (napi_get_value_bool(env, value, result) != napi_ok) {
    fail(env, "Expected boolean"); return false;
  }
  return true;
}
static bool buffer(napi_env env, napi_value value, MDB_val *result) {
  bool is_buffer;
  if (napi_is_buffer(env, value, &is_buffer) != napi_ok || !is_buffer ||
      napi_get_buffer_info(env, value, &result->mv_data, &result->mv_size) != napi_ok) {
    fail(env, "Expected Buffer"); return false;
  }
  return true;
}
static napi_value copy_buffer(napi_env env, const MDB_val *value) {
  napi_value result;
  if (napi_create_buffer_copy(env, value->mv_size, value->mv_data, NULL, &result) != napi_ok) return fail(env, "Cannot copy database value");
  return result;
}
static env_handle *environment(napi_env env, napi_value value) {
  void *pointer;
  if (napi_get_value_external(env, value, &pointer) != napi_ok || !pointer || ((env_handle *)pointer)->magic != ENV_MAGIC) {
    fail(env, "Invalid environment handle"); return NULL;
  }
  env_handle *handle = pointer;
  if (!handle->pointer) { fail(env, "Environment is closed"); return NULL; }
  return handle;
}
static txn_handle *transaction(napi_env env, napi_value value) {
  void *pointer;
  if (napi_get_value_external(env, value, &pointer) != napi_ok || !pointer || ((txn_handle *)pointer)->magic != TXN_MAGIC) {
    fail(env, "Invalid transaction handle"); return NULL;
  }
  txn_handle *handle = pointer;
  if (!handle->pointer) { fail(env, "Transaction has ended"); return NULL; }
  return handle;
}
static void release_environment(env_handle *handle) {
  if (--handle->references == 0) {
    if (handle->pointer) api_mdb_env_close(handle->pointer);
    free(handle);
  }
}
static void release_txn(txn_handle *handle) {
  handle->pointer = NULL;
  if (handle->owner) {
    handle->owner->transactions--;
    if (!handle->read_only) handle->owner->writer = false;
    release_environment(handle->owner);
    handle->owner = NULL;
  }
}
static void finalize_env(napi_env env, void *data, void *hint) {
  (void)env; (void)hint;
  env_handle *handle = data;
  release_environment(handle);
}
static void finalize_txn(napi_env env, void *data, void *hint) {
  (void)env; (void)hint;
  txn_handle *handle = data;
  if (handle->pointer) { api_mdb_txn_abort(handle->pointer); release_txn(handle); }
  free(handle);
}

static napi_value open_env(napi_env env, napi_callback_info info) {
  napi_value args[3], result;
  bool read_only; uint32_t max_dbs;
  if (!arguments(env, info, 3, args) || !flag(env, args[1], &read_only) || !uint32(env, args[2], &max_dbs)) return NULL;
  char *directory = string(env, args[0]);
  if (!directory) return NULL;
  env_handle *handle = calloc(1, sizeof(*handle));
  if (!handle) { free(directory); return fail(env, "Out of memory"); }
  handle->magic = ENV_MAGIC;
  handle->references = 1;
  int code = api_mdb_env_create(&handle->pointer);
  if (!code) code = api_mdb_env_set_maxdbs(handle->pointer, max_dbs);
  if (!code) code = api_mdb_env_open(handle->pointer, directory, MDB_NOTLS | (read_only ? MDB_RDONLY : 0), 0664);
  free(directory);
  if (code) {
    if (handle->pointer) api_mdb_env_close(handle->pointer);
    free(handle); return db_error(env, code);
  }
  if (napi_create_external(env, handle, finalize_env, NULL, &result) != napi_ok) {
    finalize_env(env, handle, NULL); return fail(env, "Cannot create environment handle");
  }
  return result;
}
static napi_value close_env(napi_env env, napi_callback_info info) {
  napi_value args[1];
  if (!arguments(env, info, 1, args)) return NULL;
  env_handle *handle = environment(env, args[0]);
  if (!handle) return NULL;
  if (handle->transactions) return fail(env, "Finish active transactions before closing the environment");
  api_mdb_env_close(handle->pointer); handle->pointer = NULL;
  return undefined(env);
}
static napi_value begin_txn(napi_env env, napi_callback_info info) {
  napi_value args[2], result; bool read_only;
  if (!arguments(env, info, 2, args) || !flag(env, args[1], &read_only)) return NULL;
  env_handle *owner = environment(env, args[0]);
  if (!owner) return NULL;
  if (!read_only && owner->writer) return fail(env, "A write transaction is already active");
  txn_handle *handle = calloc(1, sizeof(*handle));
  if (!handle) return fail(env, "Out of memory");
  handle->magic = TXN_MAGIC; handle->read_only = read_only;
  int code = api_mdb_txn_begin(owner->pointer, NULL, read_only ? MDB_RDONLY : 0, &handle->pointer);
  if (code) { free(handle); return db_error(env, code); }
  handle->owner = owner; owner->transactions++; owner->references++;
  if (!read_only) owner->writer = true;
  if (napi_create_external(env, handle, finalize_txn, NULL, &result) != napi_ok) {
    finalize_txn(env, handle, NULL); return fail(env, "Cannot create transaction handle");
  }
  return result;
}
static napi_value end_txn(napi_env env, napi_callback_info info, bool commit) {
  napi_value args[1];
  if (!arguments(env, info, 1, args)) return NULL;
  txn_handle *handle = transaction(env, args[0]);
  if (!handle) return NULL;
  int code = 0;
  if (commit) code = api_mdb_txn_commit(handle->pointer);
  else api_mdb_txn_abort(handle->pointer);
  release_txn(handle);
  if (code) return db_error(env, code);
  return undefined(env);
}
static napi_value commit_txn(napi_env env, napi_callback_info info) { return end_txn(env, info, true); }
static napi_value abort_txn(napi_env env, napi_callback_info info) { return end_txn(env, info, false); }
static napi_value database(napi_env env, napi_callback_info info) {
  napi_value args[4], result; uint32_t kind, dbi; bool create;
  if (!arguments(env, info, 4, args) || !uint32(env, args[2], &kind) || !flag(env, args[3], &create)) return NULL;
  txn_handle *handle = transaction(env, args[0]);
  if (!handle) return NULL;
  char *name = string(env, args[1]);
  if (!name) return NULL;
  int code = api_ulmdb_dbi_open(handle->pointer, name, kind, create ? MDB_CREATE : 0, &dbi);
  free(name);
  if (code) return db_error(env, code);
  if (napi_create_uint32(env, dbi, &result) != napi_ok) return fail(env, "Cannot return database handle");
  return result;
}
static napi_value get(napi_env env, napi_callback_info info) {
  napi_value args[3]; uint32_t dbi; MDB_val key, value;
  if (!arguments(env, info, 3, args) || !uint32(env, args[1], &dbi) || !buffer(env, args[2], &key)) return NULL;
  txn_handle *handle = transaction(env, args[0]);
  if (!handle) return NULL;
  int code = api_mdb_get(handle->pointer, dbi, &key, &value);
  if (code == MDB_NOTFOUND) return null_value(env);
  if (code) return db_error(env, code);
  return copy_buffer(env, &value);
}
static napi_value put(napi_env env, napi_callback_info info) {
  napi_value args[5]; uint32_t dbi, flags; MDB_val key, value;
  if (!arguments(env, info, 5, args) || !uint32(env, args[1], &dbi) || !buffer(env, args[2], &key) ||
      !buffer(env, args[3], &value) || !uint32(env, args[4], &flags)) return NULL;
  txn_handle *handle = transaction(env, args[0]);
  if (!handle) return NULL;
  int code = api_mdb_put(handle->pointer, dbi, &key, &value, flags);
  if (code) return db_error(env, code);
  return undefined(env);
}
static napi_value del(napi_env env, napi_callback_info info) {
  napi_value args[4]; uint32_t dbi; MDB_val key, value; napi_valuetype type;
  if (!arguments(env, info, 4, args) || !uint32(env, args[1], &dbi) || !buffer(env, args[2], &key)) return NULL;
  txn_handle *handle = transaction(env, args[0]);
  if (!handle) return NULL;
  if (napi_typeof(env, args[3], &type) != napi_ok) return fail(env, "Cannot read argument type");
  if (type != napi_null && !buffer(env, args[3], &value)) return NULL;
  int code = api_mdb_del(handle->pointer, dbi, &key, type == napi_null ? NULL : &value);
  if (code == MDB_NOTFOUND) return boolean(env, false);
  if (code) return db_error(env, code);
  return boolean(env, true);
}
static napi_value entries(napi_env env, napi_callback_info info) {
  napi_value args[3], result; uint32_t dbi; bool reverse;
  if (!arguments(env, info, 3, args) || !uint32(env, args[1], &dbi) || !flag(env, args[2], &reverse)) return NULL;
  txn_handle *handle = transaction(env, args[0]);
  if (!handle) return NULL;
  MDB_cursor *cursor = NULL;
  int code = api_mdb_cursor_open(handle->pointer, dbi, &cursor);
  if (code) return db_error(env, code);
  if (napi_create_array(env, &result) != napi_ok) { api_mdb_cursor_close(cursor); return fail(env, "Cannot create entries"); }
  MDB_val key, value; uint32_t index = 0;
  code = api_mdb_cursor_get(cursor, &key, &value, reverse ? 6 : 0);
  while (!code) {
    napi_value pair, jskey = copy_buffer(env, &key), jsvalue = copy_buffer(env, &value);
    if (!jskey || !jsvalue || napi_create_array_with_length(env, 2, &pair) != napi_ok ||
        napi_set_element(env, pair, 0, jskey) != napi_ok || napi_set_element(env, pair, 1, jsvalue) != napi_ok ||
        napi_set_element(env, result, index++, pair) != napi_ok) {
      api_mdb_cursor_close(cursor); return fail(env, "Cannot append entry");
    }
    code = api_mdb_cursor_get(cursor, &key, &value, reverse ? 12 : 8);
  }
  api_mdb_cursor_close(cursor);
  if (code != MDB_NOTFOUND) return db_error(env, code);
  return result;
}
static napi_value prefix_put(napi_env env, napi_callback_info info) {
  napi_value args[4]; uint32_t dbi; MDB_val value;
  if (!arguments(env, info, 4, args) || !uint32(env, args[1], &dbi) || !buffer(env, args[3], &value)) return NULL;
  txn_handle *handle = transaction(env, args[0]);
  if (!handle) return NULL;
  char *cidr = string(env, args[2]);
  if (!cidr) return NULL;
  int code = api_ulmdb_prefix_put(handle->pointer, dbi, cidr, &value);
  free(cidr);
  if (code) return db_error(env, code);
  return undefined(env);
}
static napi_value prefix_read(napi_env env, napi_callback_info info, unsigned operation) {
  napi_value args[3]; uint32_t dbi; MDB_val value, cidr;
  if (!arguments(env, info, 3, args) || !uint32(env, args[1], &dbi)) return NULL;
  txn_handle *handle = transaction(env, args[0]);
  if (!handle) return NULL;
  char *query = string(env, args[2]);
  if (!query) return NULL;
  int code;
  if (operation == 0) code = api_ulmdb_prefix_get(handle->pointer, dbi, query, &value);
  else if (operation == 1) code = api_ulmdb_prefix_delete(handle->pointer, dbi, query);
  else code = api_ulmdb_longest_match(handle->pointer, dbi, query, &cidr, &value);
  free(query);
  if (code == MDB_NOTFOUND) return operation == 1 ? boolean(env, false) : null_value(env);
  if (code) return db_error(env, code);
  if (operation == 0) return copy_buffer(env, &value);
  if (operation == 1) return boolean(env, true);
  napi_value result, prefix, bytes = copy_buffer(env, &value);
  if (!bytes || napi_create_object(env, &result) != napi_ok ||
      napi_create_string_utf8(env, cidr.mv_data, cidr.mv_size, &prefix) != napi_ok ||
      napi_set_named_property(env, result, "prefix", prefix) != napi_ok ||
      napi_set_named_property(env, result, "value", bytes) != napi_ok) return fail(env, "Cannot return prefix match");
  return result;
}
static napi_value prefix_get(napi_env env, napi_callback_info info) { return prefix_read(env, info, 0); }
static napi_value prefix_delete(napi_env env, napi_callback_info info) { return prefix_read(env, info, 1); }
static napi_value longest_match(napi_env env, napi_callback_info info) { return prefix_read(env, info, 2); }

static pthread_once_t api_once = PTHREAD_ONCE_INIT;
static char api_error[1024];
static void load_api(void) {
  const char *library_path = getenv("ULMDB_LIBRARY");
  if (!library_path || !library_path[0]) library_path = ULMDB_DEFAULT_LIBRARY;
  void *library = dlopen(library_path, RTLD_NOW | RTLD_LOCAL);
  if (!library) { snprintf(api_error, sizeof(api_error), "%s", dlerror()); return; }
  /* Keep the shared library loaded until process exit: finalizers call into it. */
#define LOAD_API(result, name, parameters) do { \
  void *symbol = dlsym(library, #name); \
  if (!symbol) { snprintf(api_error, sizeof(api_error), "ULMDB library is missing " #name); return; } \
  _Static_assert(sizeof(api_##name) == sizeof(symbol), "unsupported function pointer ABI"); \
  memcpy(&api_##name, &symbol, sizeof(symbol)); \
} while (0);
  API_LIST(LOAD_API)
}
static napi_value initialize(napi_env env, napi_value exports) {
  if (pthread_once(&api_once, load_api) != 0) return fail(env, "Cannot initialize shared-library loader");
  if (api_error[0]) return fail(env, api_error);
  napi_property_descriptor properties[] = {
    {"open", NULL, open_env, NULL, NULL, NULL, napi_default, NULL},
    {"close", NULL, close_env, NULL, NULL, NULL, napi_default, NULL},
    {"begin", NULL, begin_txn, NULL, NULL, NULL, napi_default, NULL},
    {"commit", NULL, commit_txn, NULL, NULL, NULL, napi_default, NULL},
    {"abort", NULL, abort_txn, NULL, NULL, NULL, napi_default, NULL},
    {"database", NULL, database, NULL, NULL, NULL, napi_default, NULL},
    {"get", NULL, get, NULL, NULL, NULL, napi_default, NULL},
    {"put", NULL, put, NULL, NULL, NULL, napi_default, NULL},
    {"del", NULL, del, NULL, NULL, NULL, napi_default, NULL},
    {"entries", NULL, entries, NULL, NULL, NULL, napi_default, NULL},
    {"prefixPut", NULL, prefix_put, NULL, NULL, NULL, napi_default, NULL},
    {"prefixGet", NULL, prefix_get, NULL, NULL, NULL, napi_default, NULL},
    {"prefixDelete", NULL, prefix_delete, NULL, NULL, NULL, napi_default, NULL},
    {"longestMatch", NULL, longest_match, NULL, NULL, NULL, napi_default, NULL},
  };
  if (napi_define_properties(env, exports, sizeof(properties) / sizeof(properties[0]), properties) != napi_ok) return fail(env, "Cannot initialize bindings");
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, initialize)
