
//#include <stdint.h>
//#include <stddef.h>

#if 1
// one way to define a helper function is with idex as a fixed value
static int (*trace_printk)(int, int, int, int, int) = (void *)6;
#else
// relocation is another option
extern int trace_printk(int, int, int, int, int);
#endif

typedef long long unsigned int uint64_t;
typedef long long int int64_t;
typedef unsigned char uint8_t;

typedef enum { false = 0, true } bool;

#define SIZE_PUBKEY 32
typedef struct {
    uint8_t x[SIZE_PUBKEY];
} Pubkey;

typedef struct {
    Pubkey *key;
    int64_t tokens;
    uint64_t userdata_len;
    uint8_t *userdata;
    Pubkey *program_id;
} KeyedAccounts;

// TODO support BPF function calls rather then forcing everything to be inlined

#define SOL_FN_PREFIX __attribute__((always_inline)) static

SOL_FN_PREFIX void sol_memcpy(void *src, void *dst, int len) {
    for (int i = 0; i < len; i++) {
        *((uint8_t *)dst + i) = *((uint8_t *)src + i);
    }
}

#define sol_panic() _sol_panic(__LINE__)
SOL_FN_PREFIX void _sol_panic(uint64_t line) {
    trace_printk(0, 0, 0xFF, 0xFF, line);
    char *pv = (char*)1;
    *pv = 1;
}

SOL_FN_PREFIX int deserialize(uint8_t *src, uint64_t num_ka, KeyedAccounts *ka,
                              uint64_t *userdata_len, uint8_t **userdata) {
    if (num_ka != *(uint64_t *)src) {
        return -1;
    }
    src += sizeof(uint64_t);

    for (int i = 0; i < num_ka; i++) {
        // key
        ka[i].key = (Pubkey *)src;
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
        ka[i].program_id = (Pubkey *)src;
        src += SIZE_PUBKEY;
    }
    // tx userdata
    *userdata_len = *(uint64_t *)src;
    src += sizeof(uint64_t);
    *userdata = src;

    return 0;
}

SOL_FN_PREFIX void print_params(uint64_t num_ka, KeyedAccounts *ka,
                                uint64_t userdata_len, uint8_t *userdata) {
    trace_printk(0, 0, 0, 0, 2);
    for (int i = 0; i < 2; i++) {
        // key
        for (int j = 0; j < 32; j++) {
            trace_printk(0, 0, 0, j, ka[i].key->x[j]);
        }

        // tokens
        trace_printk(0, 0, 0, 0, ka[i].tokens);

        // account userdata
        for (int j = 0; j < ka[i].userdata_len; j++) {
            trace_printk(0, 0, 0, j, ka[i].userdata[j]);
        }

        // program_id
        for (int j = 0; j < 32; j++) {
            trace_printk(0, 0, 0, j, ka[i].program_id->x[j]);
        }
    }
    // tx userdata
    for (int j = 0; j < userdata_len; j++) {
        trace_printk(0, 0, 0, j, userdata[j]);
    }
}

// void entrypoint(char *buf) {
//     KeyedAccounts ka[2];
//     uint64_t userdata_len;
//     uint8_t *userdata;

//     if (0 != deserialize((uint8_t *)buf, 2, ka, &userdata_len, &userdata)) {
//         return;
//     }

//     print_params(2, ka, userdata_len, userdata);
// }

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
    Pubkey *player_x;
    Pubkey *player_o;
    State state;
    BoardItem board[9];
    int64_t keep_alive[2];
} Game;

SOL_FN_PREFIX void game_dump_board(Game *self) {
    trace_printk(0, 0, 0x9, 0x9, 0x9);
    trace_printk(0, 0, self->board[0], self->board[1], self->board[2]);
    trace_printk(0, 0, self->board[3], self->board[4], self->board[5]);
    trace_printk(0, 0, self->board[6], self->board[7], self->board[8]);
    trace_printk(0, 0, 0x9, 0x9, 0x9);
}

SOL_FN_PREFIX void game_create(Game *self, Pubkey *player_x) {
    self->player_x = player_x;
    self->player_o = 0;
    self->state = State_Waiting;

    // TODO no loops, unroll
    for (int i = 0; i < 9; i++) {
        self->board[i] = BoardItem_F;
    }
}

SOL_FN_PREFIX Result game_join(Game *self, Pubkey *player_o, int64_t timestamp) {
    if (self->state == State_Waiting) {
        self->player_o = player_o;
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

SOL_FN_PREFIX bool game_same_player(Pubkey *one, Pubkey *two) {
    for (int i = 0; i < SIZE_PUBKEY; i++) {
        if (one->x[i] != two->x[i]) {
            return false;
        }
    }
    return true;
}

SOL_FN_PREFIX Result game_next_move(Game *self, Pubkey *player, int x, int y) {
    int board_index = y * 3 + x;
    if (board_index >= 9 || self->board[board_index] != BoardItem_F) {
        return Result_InvalidMove;
    }

    BoardItem x_or_o;
    State won_state;

    switch (self->state) {
        case State_XMove:
            if (!game_same_player(player, self->player_x)) {
                return Result_PlayerNotFound;
            }
            self->state = State_OMove;
            x_or_o = BoardItem_X;
            won_state = State_XWon;
            break;

        case State_OMove:
            if (!game_same_player(player, self->player_o)) {
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

    // TODO unroll
    {
        int draw = true;
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
    trace_printk(0, 0, 0, 0, self->state);
    return Result_Ok;
}

SOL_FN_PREFIX Result game_keep_alive(Game *self, Pubkey *player,
                                     int64_t timestamp) {
    switch (self->state) {
        case State_Waiting:
        case State_XMove:
        case State_OMove:
            if (game_same_player(player, self->player_x)) {
                if (timestamp <= self->keep_alive[0]) {
                   return Result_InvalidTimestamp;
                }
                self->keep_alive[0] = timestamp;
            } else if (game_same_player(player, self->player_o)) {
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

void entrypoint(uint8_t *buf) {
    KeyedAccounts ka[2];
    uint64_t userdata_len;
    uint8_t *userdata;

    if (0 != deserialize(buf, 2, ka, &userdata_len, &userdata)) {
        sol_panic();
    }

    // if (sizeof(Game) != userdata_len) {
    //     sol_panic();
    // }
    Game game;
    sol_memcpy(&game, userdata, userdata_len);

    game_create(&game, ka[0].key);
    game_join(&game, ka[1].key, 1);

    game_next_move(&game, ka[0].key, 1, 1);
    game_next_move(&game, ka[1].key, 0, 0);
    game_next_move(&game, ka[0].key, 2, 0);
    game_next_move(&game, ka[1].key, 0, 2);
    game_next_move(&game, ka[0].key, 2, 2);
    game_next_move(&game, ka[1].key, 0, 1);
}
