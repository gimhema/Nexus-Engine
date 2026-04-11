#include "Zone.h"

Zone::Zone(ZoneConfig config)
    : m_config(std::move(config))
{}

Vec3 Zone::PickPlayerSpawn() const
{
    if (m_config.playerSpawnPoints.empty())
        return {};

    const size_t idx = m_spawnRoundRobin % m_config.playerSpawnPoints.size();
    ++m_spawnRoundRobin;
    return m_config.playerSpawnPoints[idx].pos;
}

bool Zone::IsInBounds(const Vec3& pos) const
{
    return pos.x >= m_config.boundsMin.x && pos.x <= m_config.boundsMax.x
        && pos.y >= m_config.boundsMin.y && pos.y <= m_config.boundsMax.y
        && pos.z >= m_config.boundsMin.z && pos.z <= m_config.boundsMax.z;
}

const std::vector<NpcSpawnDef>& Zone::GetNpcSpawns() const
{
    return m_config.npcSpawns;
}
