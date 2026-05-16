#include "Item.h"
#include <ctime>


ItemUIDGenerator::ItemUIDGenerator()
	:
	m_Sequence(0),
	m_ServerId(0),
	m_Initialized(false)
{
}

ItemUIDGenerator::~ItemUIDGenerator()
{
}

ItemUIDGenerator& ItemUIDGenerator::Instance()
{
	static ItemUIDGenerator s_Instance;

	return s_Instance;
}

bool ItemUIDGenerator::Initialize(unsigned short serverId)
{
	m_ServerId = serverId;

	m_Sequence.store(0, std::memory_order_relaxed);

	m_Initialized = true;

	return true;
}

bool ItemUIDGenerator::LoadFromLastUID(uint64_t lastUID)
{
	if (m_Initialized == false)
		return false;

	unsigned short dbServerId =
		static_cast<unsigned short>((lastUID >> 48) & 0xFFFF);

	// 다른 서버 UID면 거부
	if (dbServerId != m_ServerId)
		return false;

	unsigned int seq =
		static_cast<unsigned int>(lastUID & 0xFFFF);

	m_Sequence.store(seq, std::memory_order_relaxed);

	return true;
}

uint64_t ItemUIDGenerator::Generate()
{
	if (m_Initialized == false)
		return 0;

	uint64_t uid = 0;

	uint64_t timestamp =
		static_cast<uint64_t>(time(NULL));

	uint64_t seq =
		m_Sequence.fetch_add(1, std::memory_order_relaxed);

	// overflow 방지
	seq &= 0xFFFF;

	uid |=
		(static_cast<uint64_t>(m_ServerId) << 48);

	uid |=
		((timestamp & 0xFFFFFFFF) << 16);

	uid |=
		(seq);

	return uid;
}