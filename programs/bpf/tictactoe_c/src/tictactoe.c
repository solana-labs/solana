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

// // -- Debug --

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

// -- TicTacToe --

//  Board Coodinates
// | 0,0 | 1,0 | 2,0 |
// | 0,1 | 1,1 | 2,1 |
// | 0,2 | 1,2 | 2,2 |

typedef enum {
  Result_Ok,
  Result_Panic,
  Result_GameInProgress,
  Result_InvalidArguments,
  Result_InvalidMove,
  Result_InvalidUserdata,
  Result_InvalidTimestamp,
  Result_NoGame,
  Result_NotYourTurn,
  Result_PlayerNotFound,
  Result_UserdataTooSmall,
} Result;

typedef enum { BoardItem_F, BoardItem_X, BoardItem_O } BoardItem;

typedef enum {
  State_Waiting,
  State_XMove,
  State_OMove,
  State_XWon,
  State_OWon,
  State_Draw,
} State;

typedef struct {
  SolPubkey player_x;
  SolPubkey player_o;
  State state;
  BoardItem board[9];
  int64_t keep_alive[2];
} Game;

typedef enum {
  Command_Init = 0,
  Command_Join,
  Command_KeepAlive,
  Command_Move,
} Command;

SOL_FN_PREFIX void game_dump_board(Game *self) {
  sol_print(0, 0, 0x9, 0x9, 0x9);
  sol_print(0, 0, self->board[0], self->board[1], self->board[2]);
  sol_print(0, 0, self->board[3], self->board[4], self->board[5]);
  sol_print(0, 0, self->board[6], self->board[7], self->board[8]);
  sol_print(0, 0, 0x9, 0x9, 0x9);
}

SOL_FN_PREFIX void game_create(Game *self, SolPubkey *player_x) {
  sol_memcpy(self->player_x.x, player_x, SIZE_PUBKEY);
  // TODO self->player_o = 0;
  self->state = State_Waiting;
  self->keep_alive[0] = 0;
  self->keep_alive[1] = 0;

  // TODO fixed iteration loops ok? unrolled?
  for (int i = 0; i < 9; i++) {
    self->board[i] = BoardItem_F;
  }
}

SOL_FN_PREFIX Result game_join(Game *self, SolPubkey *player_o,
                               int64_t timestamp) {
  if (self->state == State_Waiting) {
    sol_memcpy(self->player_o.x, player_o, SIZE_PUBKEY);
    self->state = State_XMove;

    if (timestamp <= self->keep_alive[1]) {
      return Result_InvalidTimestamp;
    } else {
      self->keep_alive[1] = timestamp;
      return Result_Ok;
    }
  }
  return Result_GameInProgress;
}

SOL_FN_PREFIX bool game_same(BoardItem x_or_o, BoardItem one, BoardItem two,
                             BoardItem three) {
  if (x_or_o == one && x_or_o == two && x_or_o == three) {
    return true;
  }
  return false;
}

SOL_FN_PREFIX bool game_same_player(SolPubkey *one, SolPubkey *two) {
  // TODO fixed iteration loops ok? unrolled?
  for (int i = 0; i < SIZE_PUBKEY; i++) {
    if (one->x[i] != two->x[i]) {
      return false;
    }
  }
  return true;
}

SOL_FN_PREFIX Result game_next_move(Game *self, SolPubkey *player, int x,
                                    int y) {
  int board_index = y * 3 + x;
  if (board_index >= 9 || self->board[board_index] != BoardItem_F) {
    return Result_InvalidMove;
  }

  BoardItem x_or_o;
  State won_state;

  switch (self->state) {
    case State_XMove:
      if (!game_same_player(player, &self->player_x)) {
        return Result_PlayerNotFound;
      }
      self->state = State_OMove;
      x_or_o = BoardItem_X;
      won_state = State_XWon;
      break;

    case State_OMove:
      if (!game_same_player(player, &self->player_o)) {
        return Result_PlayerNotFound;
      }
      self->state = State_XMove;
      x_or_o = BoardItem_O;
      won_state = State_OWon;
      break;

    default:
      return Result_NotYourTurn;
  }

  self->board[board_index] = x_or_o;

  // game_dump_board(self);

  bool winner =
      // Check rows
      game_same(x_or_o, self->board[0], self->board[1], self->board[2]) ||
      game_same(x_or_o, self->board[3], self->board[4], self->board[5]) ||
      game_same(x_or_o, self->board[6], self->board[7], self->board[8]) ||
      // Check columns
      game_same(x_or_o, self->board[0], self->board[3], self->board[6]) ||
      game_same(x_or_o, self->board[1], self->board[4], self->board[7]) ||
      game_same(x_or_o, self->board[2], self->board[5], self->board[8]) ||
      // Check both diagonals
      game_same(x_or_o, self->board[0], self->board[4], self->board[8]) ||
      game_same(x_or_o, self->board[2], self->board[4], self->board[6]);

  if (winner) {
    self->state = won_state;
  }

  {
    int draw = true;
    // TODO fixed iteration loops ok? unrolled?
    for (int i = 0; i < 9; i++) {
      if (BoardItem_F == self->board[i]) {
        draw = false;
        break;
      }
    }
    if (draw) {
      self->state = State_Draw;
    }
  }
  return Result_Ok;
}

SOL_FN_PREFIX Result game_keep_alive(Game *self, SolPubkey *player,
                                     int64_t timestamp) {
  switch (self->state) {
    case State_Waiting:
    case State_XMove:
    case State_OMove:
      if (game_same_player(player, &self->player_x)) {
        if (timestamp <= self->keep_alive[0]) {
          return Result_InvalidTimestamp;
        }
        self->keep_alive[0] = timestamp;
      } else if (game_same_player(player, &self->player_o)) {
        if (timestamp <= self->keep_alive[1]) {
          return Result_InvalidTimestamp;
        }
        self->keep_alive[1] = timestamp;
      } else {
        return Result_PlayerNotFound;
      }
      break;

    default:
      break;
  }
  return Result_Ok;
}

// TODO timestamp is broken

// accounts[0] On Init must be player X, after that doesn't matter,
//             anybody can cause a dashboard update
// accounts[1] must be a TicTacToe state account
// accounts[2] must be account of current player, only Pubkey is used
uint64_t entrypoint(uint8_t *buf) {
  SolKeyedAccounts ka[3];
  uint64_t tx_data_len;
  uint8_t *tx_data;
  int err = 0;

  if (1 != sol_deserialize(buf, 3, ka, &tx_data, &tx_data_len)) {
    return 0;
  }

  if (sizeof(Game) > ka[1].userdata_len) {
    sol_print(0, 0, 0xFF, sizeof(Game), ka[2].userdata_len);
    return 0;
  }
  Game game;
  sol_memcpy(&game, ka[1].userdata, ka[1].userdata_len);

  Command command = *tx_data;
  switch (command) {
    case Command_Init:
      game_create(&game, ka[2].key);
      break;

    case Command_Join:
      err = game_join(&game, ka[2].key, tx_data[8]);
      break;

    case Command_KeepAlive:
      err = game_keep_alive(&game, ka[2].key, /*TODO*/ 0);
      break;

    case Command_Move:
      err = game_next_move(&game, ka[2].key, tx_data[4], tx_data[5]);
      break;

    default:
      return 0;
  }

  sol_memcpy(ka[1].userdata, &game, ka[1].userdata_len);
  sol_print(0, 0, 0, err, game.state);
  return 1;
}
