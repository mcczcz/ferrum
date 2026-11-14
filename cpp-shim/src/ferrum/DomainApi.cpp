#include "ferrum/DomainApi.h"
#include "I18n/I18n.h"
#include "ferrum/FerrumModManager.h"
#include "ll/api/command/CommandHandle.h"
#include "ll/api/command/CommandRegistrar.h"
#include "ll/api/command/runtime/RuntimeOverload.h"
#include "ll/api/service/Bedrock.h"
#include "ll/api/utils/ErrorUtils.h"
#include "mc/legacy/ActorRuntimeID.h"
#include "mc/legacy/ActorUniqueID.h"
#include "mc/platform/UUID.h"
#include "mc/server/commands/CurrentCmdVersion.h"
#include "mc/world/actor/ActorCategory.h"
#include "mc/world/actor/player/Player.h"
#include "mc/world/attribute/AttributeInstance.h"
#include "mc/world/attribute/AttributeInstanceConstRef.h"
#include "mc/world/attribute/BaseAttributeMap.h"
#include "mc/world/attribute/MutableAttributeWithContext.h"
#include "mc/world/attribute/SharedAttributes.h"
#include "mc/world/level/BlockSource.h"
#include "mc/world/level/GameType.h"
#include "mc/world/level/Level.h"
#include "mc/world/level/block/Block.h"
#include "mc/world/level/block/BlockChangeContext.h"
#include "mc/world/level/dimension/Dimension.h"
#include "mc/world/level/dimension/DimensionType.h"
#include <atomic>
#include <cmath>
#include <map>
#include <stdexcept>
#include <unordered_map>
#include <unordered_set>

namespace ferrum {
static std::string_view view(FfiStr s) {
    return s.len ? std::string_view{reinterpret_cast<char const*>(s.ptr), s.len} : std::string_view{};
}
static FfiStr ffi(std::string_view s) { return {reinterpret_cast<uint8_t const*>(s.data()), s.size()}; }

template <class F>
static FerrumStatus guarded(F&& f) noexcept {
    try {
        return f();
    } catch (...) {
        auto detail = ll::error_utils::makeExceptionString(std::current_exception());
        ferrum_host_set_last_error(ffi(detail));
        return FerrumStatus_CPP_EXCEPTION;
    }
}
static FerrumStatus writeBytes(FfiByteWriter* out, void const* data, size_t len) {
    if (!out) return FerrumStatus_NULL_POINTER;
    return out->write_fn(out->context, static_cast<uint8_t const*>(data), len);
}
static FerrumStatus writeIds(FfiByteWriter* out, std::vector<uint64_t> const& ids) {
    // Windows x64 为小端；输出字节不暴露 std::vector 或对象指针。
    return writeBytes(out, ids.data(), ids.size() * sizeof(uint64_t));
}
static Player* resolve(ActorRuntimeId id) {
    auto level = ll::service::getLevel();
    if (!level) return nullptr;
    auto* actor = level->getRuntimeEntity(::ActorRuntimeID{id}, false);
    return actor && actor->isPlayer() ? static_cast<Player*>(actor) : nullptr;
}
static Actor* resolveActor(ActorRuntimeId id) {
    auto level = ll::service::getLevel();
    return level ? level->getRuntimeEntity(::ActorRuntimeID{id}, false) : nullptr;
}
static bool isMob(Actor const& actor) { return actor.isPlayer() || actor.hasCategory(ActorCategory::Mob); }
template <class F>
static FerrumStatus withActor(ActorRuntimeId id, F&& f) noexcept {
    return guarded([&] {
        auto* actor = resolveActor(id);
        return actor ? f(*actor) : FerrumStatus_NOT_FOUND;
    });
}
template <class F>
static FerrumStatus withMob(ActorRuntimeId id, F&& f) noexcept {
    return withActor(id, [&](Actor& actor) { return isMob(actor) ? f(actor) : FerrumStatus_INVALID_ARGUMENT; });
}
static FerrumStatus actorFind(ActorUniqueId id, ActorRuntimeId* out) noexcept {
    return guarded([&] {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto level = ll::service::getLevel();
        if (!level) return FerrumStatus_SERVICE_UNAVAILABLE;
        auto* actor = level->fetchEntity(::ActorUniqueID{id}, false);
        if (!actor) return FerrumStatus_NOT_FOUND;
        *out = actor->getRuntimeID().rawID;
        return FerrumStatus_OK;
    });
}
static FerrumStatus actorUnique(ActorRuntimeId id, ActorUniqueId* out) noexcept {
    return withActor(id, [&](Actor& actor) {
        if (!out) return FerrumStatus_NULL_POINTER;
        *out = actor.getOrCreateUniqueID().rawID;
        return FerrumStatus_OK;
    });
}
static FerrumStatus actorType(ActorRuntimeId id, FfiByteWriter* out) noexcept {
    return withActor(id, [&](Actor& actor) {
        auto name = actor.getTypeName();
        return writeBytes(out, name.data(), name.size());
    });
}
static FerrumStatus actorKind(ActorRuntimeId id, ActorKind* out) noexcept {
    return withActor(id, [&](Actor& actor) {
        if (!out) return FerrumStatus_NULL_POINTER;
        *out = actor.isPlayer() ? ActorKind_PLAYER : isMob(actor) ? ActorKind_MOB : ActorKind_ACTOR;
        return FerrumStatus_OK;
    });
}

template <class F>
static FerrumStatus withBlockSource(FerrumDimensionId dim, FerrumBlockPos pos, F&& f) noexcept {
    return guarded([&] {
        auto level = ll::service::getLevel();
        if (!level) return FerrumStatus_SERVICE_UNAVAILABLE;
        auto dimension = level->getDimension(::DimensionType{dim}).lock();
        if (!dimension) return FerrumStatus_NOT_FOUND;
        auto&      source = dimension->getBlockSourceFromMainChunkSource();
        ::BlockPos blockPos{pos.x, pos.y, pos.z};
        if (pos.y < source.getMinHeight() || pos.y >= source.getMaxHeight()) return FerrumStatus_INVALID_ARGUMENT;
        if (!source.hasChunksAt(blockPos, 0, false)) return FerrumStatus_NOT_FOUND;
        return f(source, blockPos);
    });
}
static FerrumStatus blockType(FerrumDimensionId dim, FerrumBlockPos pos, FfiByteWriter* out) noexcept {
    return withBlockSource(dim, pos, [&](BlockSource& source, ::BlockPos const& p) {
        auto name = source.getBlock(p).getTypeName();
        return writeBytes(out, name.data(), name.size());
    });
}
static FerrumStatus blockRuntime(FerrumDimensionId dim, FerrumBlockPos pos, BlockRuntimeId* out) noexcept {
    return withBlockSource(dim, pos, [&](BlockSource& source, ::BlockPos const& p) {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto const& block = source.getBlock(p);
        if (!block.mHasRuntimeId) return FerrumStatus_SERVICE_UNAVAILABLE;
        *out = block.mNetworkId;
        return FerrumStatus_OK;
    });
}
static FerrumStatus blockAir(FerrumDimensionId dim, FerrumBlockPos pos, FfiBool* out) noexcept {
    return withBlockSource(dim, pos, [&](BlockSource& source, ::BlockPos const& p) {
        if (!out) return FerrumStatus_NULL_POINTER;
        *out = source.getBlock(p).isAir() ? FfiBool_TRUE : FfiBool_FALSE;
        return FerrumStatus_OK;
    });
}
static FerrumStatus blockSetType(FerrumDimensionId dim, FerrumBlockPos pos, FfiStr name) noexcept {
    return withBlockSource(dim, pos, [&](BlockSource& source, ::BlockPos const& p) {
        auto text = view(name);
        if (text.empty()) return FerrumStatus_INVALID_ARGUMENT;
        auto block = Block::tryGetFromRegistry(HashedString{std::string{text}});
        if (!block) return FerrumStatus_NOT_FOUND;
        if (source.getBlock(p) == *block) return FerrumStatus_OK;
        BlockChangeContext context{};
        context.mContextSource         = StatelessBlockChangeContext::Scripts;
        context.mCommandVersion        = static_cast<int>(CurrentCmdVersion::Latest);
        context.mTickSchedulingEnabled = true;
        // 常规邻居更新 + 客户端同步。所有游戏版本标记均留在 shim。
        return source.setBlock(p, *block, 3, nullptr, context) ? FerrumStatus_OK : FerrumStatus_FAILED;
    });
}
static FerrumStatus findName(FfiStr name, ActorRuntimeId* out) noexcept {
    return guarded([&] {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto level = ll::service::getLevel();
        if (!level) return FerrumStatus_SERVICE_UNAVAILABLE;
        auto* player = level->getPlayer(std::string{view(name)});
        if (!player) return FerrumStatus_NOT_FOUND;
        *out = player->getRuntimeID().rawID;
        return FerrumStatus_OK;
    });
}
static mce::UUID fromUuid(PlayerUuid id) {
    uint64_t a = 0, b = 0;
    for (int i = 0; i < 8; ++i) {
        a = (a << 8) | id.bytes[i];
        b = (b << 8) | id.bytes[i + 8];
    }
    return {a, b};
}
static PlayerUuid toUuid(mce::UUID const& uuid) {
    PlayerUuid out{};
    for (int i = 0; i < 8; ++i) {
        out.bytes[i]     = static_cast<uint8_t>(uuid.a >> (56 - i * 8));
        out.bytes[i + 8] = static_cast<uint8_t>(uuid.b >> (56 - i * 8));
    }
    return out;
}
static FerrumStatus findUuid(PlayerUuid uuid, ActorRuntimeId* out) noexcept {
    return guarded([&] {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto level = ll::service::getLevel();
        if (!level) return FerrumStatus_SERVICE_UNAVAILABLE;
        auto* p = level->getPlayer(fromUuid(uuid));
        if (!p) return FerrumStatus_NOT_FOUND;
        *out = p->getRuntimeID().rawID;
        return FerrumStatus_OK;
    });
}
static FerrumStatus online(FfiByteWriter* out) noexcept {
    return guarded([&] {
        auto level = ll::service::getLevel();
        if (!level) return FerrumStatus_SERVICE_UNAVAILABLE;
        std::vector<uint64_t> ids;
        level->forEachPlayer([&](Player& p) {
            ids.push_back(p.getRuntimeID().rawID);
            return true;
        });
        return writeIds(out, ids);
    });
}
static FerrumStatus realName(ActorRuntimeId id, FfiByteWriter* out) noexcept {
    return guarded([&] {
        auto* p = resolve(id);
        if (!p) return FerrumStatus_NOT_FOUND;
        auto name = p->getRealName();
        return writeBytes(out, name.data(), name.size());
    });
}
static FerrumStatus playerUuid(ActorRuntimeId id, PlayerUuid* out) noexcept {
    return guarded([&] {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto* p = resolve(id);
        if (!p) return FerrumStatus_NOT_FOUND;
        *out = toUuid(p->getUuid());
        return FerrumStatus_OK;
    });
}
static FerrumStatus message(ActorRuntimeId id, FfiStr text) noexcept {
    return guarded([&] {
        auto* p = resolve(id);
        if (!p) return FerrumStatus_NOT_FOUND;
        p->sendMessage(view(text));
        return FerrumStatus_OK;
    });
}
static FerrumStatus isOperator(ActorRuntimeId id, FfiBool* out) noexcept {
    return guarded([&] {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto* p = resolve(id);
        if (!p) return FerrumStatus_NOT_FOUND;
        *out = p->isOperator() ? FfiBool_TRUE : FfiBool_FALSE;
        return FerrumStatus_OK;
    });
}

static FerrumStatus actorPosition(ActorRuntimeId id, FerrumVec3* out) noexcept {
    return withActor(id, [&](Actor& actor) {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto pos = actor.getPosition();
        *out     = {pos.x, pos.y, pos.z};
        return FerrumStatus_OK;
    });
}
static FerrumStatus actorDimension(ActorRuntimeId id, FerrumDimensionId* out) noexcept {
    return withActor(id, [&](Actor& actor) {
        if (!out) return FerrumStatus_NULL_POINTER;
        *out = static_cast<int>(actor.getDimensionId());
        return FerrumStatus_OK;
    });
}
static FerrumStatus actorTeleport(ActorRuntimeId id, FerrumVec3 pos, FerrumDimensionId dim) noexcept {
    return withActor(id, [&](Actor& actor) {
        if (!std::isfinite(pos.x) || !std::isfinite(pos.y) || !std::isfinite(pos.z))
            return FerrumStatus_INVALID_ARGUMENT;
        auto level = ll::service::getLevel();
        if (!level) return FerrumStatus_SERVICE_UNAVAILABLE;
        if (!level->getDimension(::DimensionType{dim}).lock()) return FerrumStatus_NOT_FOUND;
        actor.teleport(::Vec3{pos.x, pos.y, pos.z}, ::DimensionType{dim});
        return FerrumStatus_OK;
    });
}
static FerrumStatus mobHealth(ActorRuntimeId id, float* out) noexcept {
    return withMob(id, [&](Actor& actor) {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto        attribute = actor.getAttribute(SharedAttributes::HEALTH());
        auto const* instance  = static_cast<AttributeInstance const*>(attribute.mPtr);
        if (!instance) return FerrumStatus_UNSUPPORTED;
        *out = instance->mCurrentValue;
        return FerrumStatus_OK;
    });
}
static FerrumStatus mobMaxHealth(ActorRuntimeId id, float* out) noexcept {
    return withMob(id, [&](Actor& actor) {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto        attribute = actor.getAttribute(SharedAttributes::HEALTH());
        auto const* instance  = static_cast<AttributeInstance const*>(attribute.mPtr);
        if (!instance) return FerrumStatus_UNSUPPORTED;
        *out = instance->mCurrentMaxValue;
        return FerrumStatus_OK;
    });
}
static FerrumStatus mobSetHealth(ActorRuntimeId id, float value) noexcept {
    return withMob(id, [&](Actor& actor) {
        if (!std::isfinite(value) || value < 0) return FerrumStatus_INVALID_ARGUMENT;
        {
            auto        attribute = actor.getAttribute(SharedAttributes::HEALTH());
            auto const* instance  = static_cast<AttributeInstance const*>(attribute.mPtr);
            if (!instance) return FerrumStatus_UNSUPPORTED;
            if (value > static_cast<float>(instance->mCurrentMaxValue)) return FerrumStatus_INVALID_ARGUMENT;
        }
        auto  attribute = actor.getMutableAttribute(SharedAttributes::HEALTH());
        auto* map       = static_cast<BaseAttributeMap*>(attribute.mContext->mAttributeMap);
        if (!map) return FerrumStatus_UNSUPPORTED;
        return map->setCurrentValue(SharedAttributes::HEALTH(), value) ? FerrumStatus_OK : FerrumStatus_FAILED;
    });
}
// 保留开发期已有玩家 ABI 槽位，转发到相同公共实现，玩家类型仍逐次校验。
template <class F>
static FerrumStatus withPlayer(ActorRuntimeId id, F&& f) noexcept {
    return guarded([&] { return resolve(id) ? f() : FerrumStatus_NOT_FOUND; });
}
static FerrumStatus playerPosition(ActorRuntimeId id, FerrumVec3* out) noexcept {
    return withPlayer(id, [&] { return actorPosition(id, out); });
}
static FerrumStatus playerDimension(ActorRuntimeId id, FerrumDimensionId* out) noexcept {
    return withPlayer(id, [&] { return actorDimension(id, out); });
}
static FerrumStatus playerTeleport(ActorRuntimeId id, FerrumVec3 pos, FerrumDimensionId dim) noexcept {
    return withPlayer(id, [&] { return actorTeleport(id, pos, dim); });
}
static FerrumStatus playerHealth(ActorRuntimeId id, float* out) noexcept {
    return withPlayer(id, [&] { return mobHealth(id, out); });
}
static FerrumStatus playerMaxHealth(ActorRuntimeId id, float* out) noexcept {
    return withPlayer(id, [&] { return mobMaxHealth(id, out); });
}
static FerrumStatus playerSetHealth(ActorRuntimeId id, float value) noexcept {
    return withPlayer(id, [&] { return mobSetHealth(id, value); });
}
static FerrumStatus playerGameMode(ActorRuntimeId id, FerrumGameMode* out) noexcept {
    return guarded([&] {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto* p = resolve(id);
        if (!p) return FerrumStatus_NOT_FOUND;
        switch (p->getPlayerGameType()) {
        case GameType::Survival:
            *out = FerrumGameMode_SURVIVAL;
            break;
        case GameType::Creative:
            *out = FerrumGameMode_CREATIVE;
            break;
        case GameType::Adventure:
            *out = FerrumGameMode_ADVENTURE;
            break;
        case GameType::Spectator:
            *out = FerrumGameMode_SPECTATOR;
            break;
        default:
            *out = FerrumGameMode_UNKNOWN;
            break;
        }
        return FerrumStatus_OK;
    });
}
static FerrumStatus playerSetGameMode(ActorRuntimeId id, FerrumGameMode mode) noexcept {
    return guarded([&] {
        ::GameType gameType;
        switch (mode) {
        case FerrumGameMode_SURVIVAL:
            gameType = GameType::Survival;
            break;
        case FerrumGameMode_CREATIVE:
            gameType = GameType::Creative;
            break;
        case FerrumGameMode_ADVENTURE:
            gameType = GameType::Adventure;
            break;
        case FerrumGameMode_SPECTATOR:
            gameType = GameType::Spectator;
            break;
        default:
            return FerrumStatus_INVALID_ARGUMENT;
        }
        auto* p = resolve(id);
        if (!p) return FerrumStatus_NOT_FOUND;
        p->setPlayerGameType(gameType);
        return FerrumStatus_OK;
    });
}

struct CommandRoute {
    CommandPermission                                           permission{};
    std::string                                                 owner;
    std::atomic<uint64_t>                                       handle{0};
    std::unordered_map<std::string, ll::command::ParamKindType> params;
};
static std::map<std::string, std::map<std::string, std::shared_ptr<CommandRoute>>> routes;
struct CommandScope {
    ActorRuntimeId       originPlayer{};
    CommandOrigin const& origin;
    CommandOutput&       output;
    struct Param {
        ll::command::ParamKindType                                                          kind;
        std::optional<std::variant<int, float, bool, std::string, CommandSelector<Player>>> value;
    };
    std::unordered_map<std::string, Param> params;
};
static thread_local std::unordered_map<uint64_t, CommandScope*> scopes;
static thread_local uint64_t                                    nextScope = 0;

static ll::command::ParamKindType paramKind(CommandParamKind kind) {
    using namespace ll::command;
    if (kind == CommandParamKind_INT) return ParamKind::Int;
    if (kind == CommandParamKind_FLOAT) return ParamKind::Float;
    if (kind == CommandParamKind_BOOL) return ParamKind::Bool;
    if (kind == CommandParamKind_STRING) return ParamKind::String;
    if (kind == CommandParamKind_PLAYER) return ParamKind::Player;
    throw std::invalid_argument("unsupported command parameter kind");
}
static bool identifier(std::string_view s) {
    if (s.empty()) return false;
    for (auto ch : s)
        if (!((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9') || ch == '_'
              || ch == '-'))
            return false;
    return true;
}
static FerrumStatus
registerCommand(ShimModCookie cookie, ::CommandHandle handle, CommandDefinitionV1 const* def) noexcept {
    return guarded([&] {
        if (!def || (!def->params.ptr && def->params.len)) return FerrumStatus_NULL_POINTER;
        auto owner = *static_cast<std::shared_ptr<FerrumMod>*>(cookie);
        auto name  = std::string{view(def->name)};
        if (!identifier(name) || def->permission > CommandPermission_OWNER) return FerrumStatus_INVALID_ARGUMENT;
        bool                            optional = false;
        std::unordered_set<std::string> names;
        std::string                     signature;
        for (size_t i = 0; i < def->params.len; ++i) {
            auto const& p         = def->params.ptr[i];
            auto        paramName = std::string{view(p.name)};
            if (!identifier(paramName) || !names.insert(paramName).second || (!p.optional && optional)
                || p.kind > CommandParamKind_PLAYER)
                return FerrumStatus_INVALID_ARGUMENT;
            optional   = optional || p.optional;
            signature += std::to_string(p.kind) + ":" + paramName + ":" + std::to_string(p.optional) + ";";
        }
        auto registry = ll::service::getCommandRegistry();
        if (!registry) return FerrumStatus_SERVICE_UNAVAILABLE;
        auto existing = routes.find(name);
        if (existing == routes.end() && registry->findCommand(name)) return FerrumStatus_ALREADY_EXISTS;
        if (existing != routes.end()) {
            for (auto const& [_, r] : existing->second) {
                if (r->owner != owner->getName()) return FerrumStatus_PERMISSION_DENIED;
                if (r->permission != def->permission) return FerrumStatus_INVALID_ARGUMENT;
            }
        }
        auto& slot = routes[name][signature];
        if (slot && slot->handle.load() != 0) return FerrumStatus_ALREADY_EXISTS;
        if (!slot) {
            slot             = std::make_shared<CommandRoute>();
            slot->owner      = owner->getName();
            slot->permission = def->permission;
        }
        auto  route   = slot;
        auto& command = ll::command::CommandRegistrar::getServerInstance().getOrCreateCommand(
            name,
            std::string{view(def->description)},
            static_cast<CommandPermissionLevel>(def->permission),
            CommandFlagValue::NotCheat,
            owner
        );
        auto overload = command.runtimeOverload(owner);
        for (size_t i = 0; i < def->params.len; ++i) {
            auto const& p = def->params.ptr[i];
            route->params.emplace(std::string{view(p.name)}, paramKind(p.kind));
            if (p.optional) (void)overload.optional(view(p.name), paramKind(p.kind));
            else (void)overload.required(view(p.name), paramKind(p.kind));
        }
        overload.execute(
            [route](CommandOrigin const& origin, CommandOutput& output, ll::command::RuntimeCommand const& cmd) {
                (void)guarded([&] {
                    if (!ferrum_host_is_main_thread()) {
                        output.error(tr("command.wrong_thread"));
                        return FerrumStatus_WRONG_THREAD;
                    }
                    // 拷贝强引用，执行期间即使 LL 停用或替换工厂也不再访问捕获环境。
                    auto activeRoute = route;
                    auto raw         = activeRoute->handle.load();
                    if (!raw) return FerrumStatus_MOD_DISABLED;
                    // 宿主在授予令牌前核对已记录的服务端线程，绝不在此重标主线程。
                    ActorRuntimeId originId = 0;
                    if (auto* actor = origin.getEntity(); actor && actor->isPlayer())
                        originId = actor->getRuntimeID().rawID;
                    CommandScope scope{originId, origin, output, {}};
                    // LL 停用命令会释放工厂，故调用模组前先复制参数，之后不再访问 cmd。
                    for (auto const& [name, kind] : activeRoute->params) {
                        auto const&         value = cmd[name];
                        CommandScope::Param param{kind, {}};
                        if (value.has_value()) {
                            using namespace ll::command;
                            switch (kind) {
                            case ParamKind::Int:
                                param.value = value.get<ParamKind::Int>();
                                break;
                            case ParamKind::Float:
                                param.value = value.get<ParamKind::Float>();
                                break;
                            case ParamKind::Bool:
                                param.value = value.get<ParamKind::Bool>();
                                break;
                            case ParamKind::String:
                                param.value = value.get<ParamKind::String>();
                                break;
                            case ParamKind::Player:
                                param.value = value.get<ParamKind::Player>();
                                break;
                            }
                        }
                        scope.params.emplace(name, std::move(param));
                    }
                    auto id = ++nextScope;
                    scopes.emplace(id, &scope);
                    auto status = ferrum_host_dispatch_command(raw, id);
                    scopes.erase(id);
                    if (status != FerrumStatus_OK) {
                        output.error(tr("command.failed", status));
                    }
                    return status;
                });
            }
        );
        route->handle.store(handle);
        return FerrumStatus_OK;
    });
}
static FerrumStatus unregisterCommand(ShimModCookie cookie, ::CommandHandle handle) noexcept {
    return guarded([&] {
        auto owner = *static_cast<std::shared_ptr<FerrumMod>*>(cookie);
        for (auto const& [_, overloads] : routes)
            for (auto const& [__, route] : overloads) {
                if (route->handle.load() == handle) {
                    if (route->owner != owner->getName()) return FerrumStatus_PERMISSION_DENIED;
                    route->handle.store(0);
                    return FerrumStatus_OK;
                }
            }
        return FerrumStatus_STALE_HANDLE;
    });
}
template <class F>
static FerrumStatus withScope(uint64_t id, F&& f) noexcept {
    return guarded([&] {
        auto it = scopes.find(id);
        return it == scopes.end() ? FerrumStatus_STALE_HANDLE : f(*it->second);
    });
}
static FerrumStatus originPlayer(uint64_t id, ActorRuntimeId* out) noexcept {
    return withScope(id, [&](CommandScope& s) {
        if (!out) return FerrumStatus_NULL_POINTER;
        if (!s.originPlayer || !resolve(s.originPlayer)) return FerrumStatus_NOT_FOUND;
        *out = s.originPlayer;
        return FerrumStatus_OK;
    });
}
template <size_t Kind, class Out>
static FerrumStatus number(uint64_t id, FfiStr name, Out* out) noexcept {
    return withScope(id, [&](CommandScope& s) {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto found = s.params.find(std::string{view(name)});
        if (found == s.params.end() || found->second.kind != Kind) return FerrumStatus_INVALID_ARGUMENT;
        if (!found->second.value) return FerrumStatus_NOT_FOUND;
        using Value = ll::command::ParamKindList::get<Kind>;
        *out        = static_cast<Out>(std::get<Value>(*found->second.value));
        return FerrumStatus_OK;
    });
}
static FerrumStatus intParam(uint64_t id, FfiStr n, int32_t* out) noexcept {
    return number<ll::command::ParamKind::Int>(id, n, out);
}
static FerrumStatus floatParam(uint64_t id, FfiStr n, float* out) noexcept {
    return number<ll::command::ParamKind::Float>(id, n, out);
}
static FerrumStatus boolParam(uint64_t id, FfiStr n, FfiBool* out) noexcept {
    return number<ll::command::ParamKind::Bool>(id, n, out);
}
static FerrumStatus stringParam(uint64_t id, FfiStr name, FfiByteWriter* out) noexcept {
    return withScope(id, [&](CommandScope& s) {
        auto found = s.params.find(std::string{view(name)});
        if (found == s.params.end() || found->second.kind != ll::command::ParamKind::String)
            return FerrumStatus_INVALID_ARGUMENT;
        if (!found->second.value) return FerrumStatus_NOT_FOUND;
        auto text = std::get<std::string>(*found->second.value);
        return writeBytes(out, text.data(), text.size());
    });
}
static FerrumStatus playersParam(uint64_t id, FfiStr name, FfiByteWriter* out) noexcept {
    return withScope(id, [&](CommandScope& s) {
        auto found = s.params.find(std::string{view(name)});
        if (found == s.params.end() || found->second.kind != ll::command::ParamKind::Player)
            return FerrumStatus_INVALID_ARGUMENT;
        if (!found->second.value) return FerrumStatus_NOT_FOUND;
        if (s.originPlayer && !resolve(s.originPlayer)) return FerrumStatus_NOT_FOUND;
        std::vector<uint64_t> ids;
        {
            auto result = std::get<CommandSelector<Player>>(*found->second.value).results(s.origin);
            for (auto* player : result) ids.push_back(player->getRuntimeID().rawID);
        }
        return writeIds(out, ids);
    });
}
static FerrumStatus commandOutput(uint64_t id, FfiBool error, FfiStr text) noexcept {
    return withScope(id, [&](CommandScope& s) {
        if (error) s.output.error(view(text));
        else s.output.success(view(text));
        return FerrumStatus_OK;
    });
}
void installDomainApi(FerrumShimApi& api) {
    api.actor_v1 = {
        sizeof(ActorApiV1),
        actorFind,
        actorUnique,
        actorType,
        actorKind,
        actorPosition,
        actorDimension,
        actorTeleport
    };
    api.mob_v1             = {sizeof(MobApiV1), mobHealth, mobMaxHealth, mobSetHealth};
    api.block_v1           = {sizeof(BlockApiV1), blockType, blockRuntime, blockAir, blockSetType};
    api.command_register   = registerCommand;
    api.command_unregister = unregisterCommand;
    api.command_origin     = originPlayer;
    api.command_int        = intParam;
    api.command_float      = floatParam;
    api.command_bool       = boolParam;
    api.command_string     = stringParam;
    api.command_players    = playersParam;
    api.command_output     = commandOutput;
    api.player_v1          = {
        sizeof(PlayerApiV1),
        findName,
        findUuid,
        online,
        realName,
        playerUuid,
        message,
        isOperator,
        playerPosition,
        playerDimension,
        playerTeleport,
        playerHealth,
        playerMaxHealth,
        playerSetHealth,
        playerGameMode,
        playerSetGameMode
    };
}
} // namespace ferrum
