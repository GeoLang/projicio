TEST(operation, projCRS_to_projCRS_context_incompatible_areas_ballpark) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem("26711"), // UTM 11 NAD27
        authFactory->createCoordinateReferenceSystem(
            "3034"), // ETRS89 / LCC Europe
        ctxt);
    ASSERT_GE(list.size(), 1U);
    EXPECT_TRUE(list[0]->hasBallparkTransformation());
}

