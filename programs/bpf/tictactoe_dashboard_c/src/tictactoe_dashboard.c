//#include <stdint.h>
//#include <stddef.h>

#if 1
#define BPF_TRACE_PRINTK_IDX 6
static int (*sol_print)(int, int, int, int, int) = (void *)BPF_TRACE_PRINTK_IDX;
#else
// relocation is another option
extern int sol_print(int, int, int, int, int);
#endif

typedef long long unsigned int uint64_t;
typedef long long int int64_t;
typedef long unsigned int uint32_t;
typedef long int int32_t;
typedef unsigned char uint8_t;

typedef enum { false = 0, true } bool;

// TODO support BPF function calls rather then forcing everything to be inlined
#define SOL_FN_PREFIX __attribute__((always_inline)) static

// TODO move this to a registered helper
SOL_FN_PREFIX void sol_memcpy(void *dst, void *src, int len) {
  for (int i = 0; i < len; i++) {
    *((uint8_t *)dst + i) = *((uint8_t *)src + i);
  }
}

#define sol_trace() sol_print(0, 0, 0xFF, 0xFF, (__LINE__));
#define sol_panic() _sol_panic(__LINE__)
SOL_FN_PREFIX void _sol_panic(uint64_t line) {
  sol_print(0, 0, 0xFF, 0xFF, line);
  char *pv = (char *)1;
  *pv = 1;
}

#define SIZE_PUBKEY 32
typedef struct {
  uint8_t x[SIZE_PUBKEY];
} SolPubkey;

SOL_FN_PREFIX bool SolPubkey_same(SolPubkey *one, SolPubkey *two) {
  for (int i = 0; i < SIZE_PUBKEY; i++) {
    if (one->x[i] != two->x[i]) {
      return false;
    }
  }
  return true;
}

typedef struct {
  SolPubkey *key;
  int64_t tokens;
  uint64_t userdata_len;
  uint8_t *userdata;
  SolPubkey *program_id;
} SolKeyedAccounts;

SOL_FN_PREFIX int sol_deserialize(uint8_t *src, uint64_t num_ka,
                                  SolKeyedAccounts *ka, uint8_t **tx_data,
                                  uint64_t *tx_data_len) {
  if (num_ka != *(uint64_t *)src) {
    return 0;
  }
  src += sizeof(uint64_t);

  // TODO fixed iteration loops ok? unrolled?
  for (int i = 0; i < num_ka;
       i++) {  // TODO this should end up unrolled, confirm
    // key
    ka[i].key = (SolPubkey *)src;
    src += SIZE_PUBKEY;

    // tokens
    ka[i].tokens = *(uint64_t *)src;
    src += sizeof(uint64_t);

    // account userdata
    ka[i].userdata_len = *(uint64_t *)src;
    src += sizeof(uint64_t);
    ka[i].userdata = src;
    src += ka[i].userdata_len;

    // program_id
    ka[i].program_id = (SolPubkey *)src;
    src += SIZE_PUBKEY;
  }
  // tx userdata
  *tx_data_len = *(uint64_t *)src;
  src += sizeof(uint64_t);
  *tx_data = src;

  return 1;
}

// -- Debug --

SOL_FN_PREFIX void print_key(SolPubkey *key) {
  for (int j = 0; j < SIZE_PUBKEY; j++) {
    sol_print(0, 0, 0, j, key->x[j]);
  }
}

SOL_FN_PREFIX void print_data(uint8_t *data, int len) {
  for (int j = 0; j < len; j++) {
    sol_print(0, 0, 0, j, data[j]);
  }
}

SOL_FN_PREFIX void print_params(uint64_t num_ka, SolKeyedAccounts *ka,
                                uint8_t *tx_data, uint64_t tx_data_len) {
  sol_print(0, 0, 0, 0, num_ka);
  for (int i = 0; i < num_ka; i++) {
    // key
    print_key(ka[i].key);

    // tokens
    sol_print(0, 0, 0, 0, ka[i].tokens);

    // account userdata
    print_data(ka[i].userdata, ka[i].userdata_len);

    // program_id
    print_key(ka[i].program_id);
  }
  // tx userdata
  print_data(tx_data, tx_data_len);
}

// -- TicTacToe Dashboard --

// TODO put this in a common place for both tictactoe and tictactoe_dashboard
typedef enum {
  State_Waiting,
  State_XMove,
  State_OMove,
  State_XWon,
  State_OWon,
  State_Draw,
} State;

// TODO put this in a common place for both tictactoe and tictactoe_dashboard
typedef enum { BoardItem_F, BoardItem_X, BoardItem_O } BoardItem;

// TODO put this in a common place for both tictactoe and tictactoe_dashboard
typedef struct {
  SolPubkey player_x;
  SolPubkey player_o;
  State state;
  BoardItem board[9];
  int64_t keep_alive[2];
} Game;

#define MAX_GAMES_TRACKED 5

typedef struct {
  // Latest pending game
  SolPubkey pending;
  // Last N completed games (0 is the latest)
  SolPubkey completed[MAX_GAMES_TRACKED];
  // Index into completed pointing to latest game completed
  uint32_t latest_game;
  // Total number of completed games
  uint32_t total;
} Dashboard;

SOL_FN_PREFIX bool update(Dashboard *self, Game *game, SolPubkey *game_pubkey) {
  switch (game->state) {
    case State_Waiting:
      sol_memcpy(&self->pending, game_pubkey, SIZE_PUBKEY);
      break;
    case State_XMove:
    case State_OMove:
      // Nothing to do.  In progress games are not managed by the dashboard
      break;
    case State_XWon:
    case State_OWon:
    case State_Draw:
      for (int i = 0; i < MAX_GAMES_TRACKED; i++) {
        if (SolPubkey_same(&self->completed[i], game_pubkey)) {
          // TODO: Once the PoH height is exposed to programs, it could be used
          // to ensure
          //       that old games are not being re-added and causing total to
          //       increment incorrectly.
          return false;
        }
      }
      self->total += 1;
      self->latest_game = (self->latest_game + 1) % MAX_GAMES_TRACKED;
      sol_memcpy(self->completed[self->latest_game].x, game_pubkey,
                 SIZE_PUBKEY);
      break;

    default:
      break;
  }
  return true;
}

// accounts[0] doesn't matter, anybody can cause a dashboard update
// accounts[1] must be a Dashboard account
// accounts[2] must be a Game account
uint64_t entrypoint(uint8_t *buf) {
  SolKeyedAccounts ka[3];
  uint64_t tx_data_len;
  uint8_t *tx_data;
  int err = 0;

  if (1 != sol_deserialize(buf, 3, ka, &tx_data, &tx_data_len)) {
    return false;
  }

  // TODO check dashboard and game program ids (how to check now that they are
  // not know values)
  // TODO check validity of dashboard and game structures contents
  if (sizeof(Dashboard) > ka[1].userdata_len) {
    sol_print(0, 0, 0xFF, sizeof(Dashboard), ka[2].userdata_len);
    return false;
  }
  Dashboard dashboard;
  sol_memcpy(&dashboard, ka[1].userdata, sizeof(dashboard));

  if (sizeof(Game) > ka[2].userdata_len) {
    sol_print(0, 0, 0xFF, sizeof(Game), ka[2].userdata_len);
    return false;
  }
  Game game;
  sol_memcpy(&game, ka[2].userdata, sizeof(game));
  if (true != update(&dashboard, &game, ka[2].key)) {
    return false;
  }

  sol_memcpy(ka[1].userdata, &dashboard, sizeof(dashboard));
  return true;
}
