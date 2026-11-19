#pragma once
#include "ferrum/ferrum_abi.h"

#ifdef LL_PLAT_S
class Player;
#endif

namespace ferrum {
void installDomainApi(FerrumShimApi& api);
LlMoneyApiV1 const* llMoneyApi() noexcept;
#ifdef LL_PLAT_S
FerrumStatus formPlayerDisconnected(Player& player) noexcept;
#endif
}
