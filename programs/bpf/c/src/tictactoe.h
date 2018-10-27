#ifndef TICTACTOE_H
#define TICTACTOE_H
/**
 * @brief Definitions common to tictactoe and tictactoe_dashboard
 */

typedef enum {
  State_Waiting,
  State_XMove,
  State_OMove,
  State_XWon,
  State_OWon,
  State_Draw,
} State;

typedef enum { BoardItem_F, BoardItem_X, BoardItem_O } BoardItem;

/**
 * Game state
 *
 * This structure is stored in the owner's account userdata
 *
 * Board Coordinates
 * | 0,0 | 1,0 | 2,0 |
 * | 0,1 | 1,1 | 2,1 |
 * | 0,2 | 1,2 | 2,2 |
 */
typedef struct {
  SolPubkey player_x;    /** Player who initialized the game */
  SolPubkey player_o;    /** Player who joined the game */
  State state;           /** Current state of the game */
  BoardItem board[9];    /** Tracks the player moves */
  int64_t keep_alive[2]; /** Keep Alive for each player */
} Game;

#endif // TICTACTOE_H
